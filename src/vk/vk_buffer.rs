use std::{cell::Cell, marker::PhantomData, ops::Deref, rc::Rc};

use ash::{prelude::VkResult, vk};
#[allow(unused_imports)]
use log::trace;

use crate::AnyResult;

use super::{
        vk_command_buffer::VkReusableCommandBuffer,
        vk_wrapper::{
                impl_destroyable_deref, impl_destroyable_drop, impl_destroyable_expr, VkPhysicalDevice, VmaAllocator,
        },
};

#[derive(Clone)]
pub struct VkBufferCreateInfo<'a> {
        pub device: &'a ash::Device,
        pub allocator: Rc<VmaAllocator>,

        pub buffer_size: vk::DeviceSize,
        pub buffer_usage: vk::BufferUsageFlags,
        pub mem_usage: vma::MemoryUsage,
        pub alloc_flags: vma::AllocationCreateFlags,
        pub req_mem_flags: vk::MemoryPropertyFlags,
        pub pref_mem_flags: vk::MemoryPropertyFlags,
        pub mem_type_bits: u32,
        pub q_family_indices: Option<&'a [u32]>,
}

#[allow(dead_code)]
pub enum BufferData<'a, T> {
        FullSlice(&'a [T]),
        OffsetLength {
                data: &'a [T],
                offset: usize,
                length: usize,
        },
}

pub struct VkImmutableBufferCreateInfo<'a, T> {
        pub device: &'a ash::Device,
        pub allocator: Rc<VmaAllocator>,

        pub cmd_buffer: &'a VkReusableCommandBuffer,
        pub transfer_queue: vk::Queue,
        pub buffer_usage: vk::BufferUsageFlags,
        pub data: BufferData<'a, T>,
}

pub struct VkBuffer {
        allocator: Rc<VmaAllocator>,

        handle: vk::Buffer,
        alloc: vma::Allocation,
        ainfo: vma::AllocationInfo,
        destroyed: Cell<bool>,

        memory: Cell<*mut u8>,
}

impl VkBuffer {
        pub fn new(create_info: VkBufferCreateInfo) -> AnyResult<Self> {
                let mut handle_cinfo = vk::BufferCreateInfo::builder()
                        .size(create_info.buffer_size)
                        .usage(create_info.buffer_usage);

                match create_info.q_family_indices {
                        Some(q_family_indices) => {
                                handle_cinfo = handle_cinfo
                                        .sharing_mode(vk::SharingMode::CONCURRENT)
                                        .queue_family_indices(q_family_indices)
                        },
                        None => handle_cinfo = handle_cinfo.sharing_mode(vk::SharingMode::EXCLUSIVE),
                };

                let alloc_cinfo = vma::AllocationCreateInfo {
                        usage: create_info.mem_usage,
                        flags: create_info.alloc_flags,
                        required_flags: create_info.req_mem_flags,
                        preferred_flags: create_info.pref_mem_flags,
                        memory_type_bits: create_info.mem_type_bits,
                        pool: None,
                        user_data: None,
                        priority: 0.0,
                };

                let (handle, alloc, ainfo) =
                        unsafe { create_info.allocator.create_buffer(&handle_cinfo, &alloc_cinfo)? };

                Ok(Self {
                        allocator: create_info.allocator,
                        handle,
                        alloc,
                        ainfo,
                        destroyed: Cell::new(false),
                        memory: Cell::new(std::ptr::null_mut()),
                })
        }

        pub fn new_immutable<T>(create_info: VkImmutableBufferCreateInfo<T>) -> AnyResult<Self> {
                let buffer_data = match create_info.data {
                        BufferData::FullSlice(s) => unsafe {
                                std::slice::from_raw_parts(s.as_ptr() as *const u8, s.len() * std::mem::size_of::<T>())
                        },
                        BufferData::OffsetLength { data, offset, length } => unsafe {
                                assert!((offset + length) <= (data.len() * std::mem::size_of::<T>()));

                                std::slice::from_raw_parts((data.as_ptr() as *const u8).offset(offset as isize), length)
                        },
                };
                let buffer_size = buffer_data.len() as vk::DeviceSize;

                let staging_buffer_cinfo = VkBufferCreateInfo {
                        device: create_info.device,
                        allocator: Rc::clone(&create_info.allocator),
                        buffer_size,
                        buffer_usage: vk::BufferUsageFlags::TRANSFER_SRC,
                        mem_usage: vma::MemoryUsage::CpuOnly,
                        alloc_flags: vma::AllocationCreateFlags::NONE,
                        req_mem_flags: vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                        pref_mem_flags: Default::default(),
                        mem_type_bits: 0,
                        q_family_indices: None,
                };

                let staging_buffer = VkBuffer::new(staging_buffer_cinfo)?;

                staging_buffer.write_bytes(buffer_data)?;
                staging_buffer.unmap_memory();

                let buffer_cinfo = VkBufferCreateInfo {
                        device: create_info.device,
                        allocator: create_info.allocator,
                        buffer_size,
                        buffer_usage: vk::BufferUsageFlags::TRANSFER_DST | create_info.buffer_usage,
                        mem_usage: vma::MemoryUsage::GpuOnly,
                        alloc_flags: vma::AllocationCreateFlags::NONE,
                        req_mem_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                        pref_mem_flags: Default::default(),
                        mem_type_bits: 0,
                        q_family_indices: None,
                };
                let buffer = VkBuffer::new(buffer_cinfo)?;

                create_info.cmd_buffer.record_and_submit(
                        create_info.transfer_queue,
                        &[],
                        &[],
                        &[],
                        |device, cmd_buffer| unsafe {
                                let regions = [vk::BufferCopy {
                                        src_offset: 0,
                                        dst_offset: 0,
                                        size: buffer_size,
                                }];

                                device.cmd_copy_buffer(cmd_buffer, staging_buffer.handle, buffer.handle, &regions);

                                Ok(())
                        },
                )?;

                unsafe {
                        create_info.cmd_buffer.wait(u64::MAX)?;
                        staging_buffer.destroy();
                }

                Ok(buffer)
        }

        pub fn new_uniform_buffer(
                device: &ash::Device,
                allocator: Rc<VmaAllocator>,
                buffer_size: vk::DeviceSize,
        ) -> AnyResult<Self> {
                let cinfo = VkBufferCreateInfo {
                        device,
                        allocator,
                        buffer_size,
                        buffer_usage: vk::BufferUsageFlags::UNIFORM_BUFFER,
                        mem_usage: vma::MemoryUsage::CpuToGpu,
                        alloc_flags: vma::AllocationCreateFlags::NONE,
                        req_mem_flags: vk::MemoryPropertyFlags::HOST_COHERENT | vk::MemoryPropertyFlags::HOST_VISIBLE,
                        pref_mem_flags: Default::default(),
                        mem_type_bits: 0,
                        q_family_indices: None,
                };

                VkBuffer::new(cinfo)
        }

        pub fn new_transfer_src(
                device: &ash::Device,
                allocator: Rc<VmaAllocator>,
                buffer_size: vk::DeviceSize,
        ) -> AnyResult<VkBuffer> {
                let staging_buffer = {
                        let buffer_cinfo = VkBufferCreateInfo {
                                device,
                                allocator,
                                buffer_size,
                                buffer_usage: vk::BufferUsageFlags::TRANSFER_SRC,
                                mem_usage: vma::MemoryUsage::CpuOnly,
                                alloc_flags: vma::AllocationCreateFlags::NONE,
                                req_mem_flags: vk::MemoryPropertyFlags::HOST_VISIBLE
                                        | vk::MemoryPropertyFlags::HOST_COHERENT,
                                pref_mem_flags: Default::default(),
                                mem_type_bits: 0,
                                q_family_indices: None,
                        };

                        VkBuffer::new(buffer_cinfo)?
                };

                Ok(staging_buffer)
        }

        #[allow(dead_code)]
        pub fn write<T: 'static>(&self, value: &T) -> VkResult<()> {
                self.write_offsetted(value, 0)
        }

        #[allow(dead_code)]
        pub fn write_offsetted<T: 'static>(&self, value: &T, offset: usize) -> VkResult<()> {
                let data = value as *const _ as *const u8;
                let len = std::mem::size_of::<T>();
                let bytes = unsafe { std::slice::from_raw_parts(data, len) };

                self.write_bytes_offsetted(bytes, offset)
        }

        #[allow(dead_code)]
        pub fn write_slice<T: 'static>(&self, data: &[T]) -> VkResult<()> {
                self.write_slice_offsetted(data, 0)
        }

        #[allow(dead_code)]
        pub fn write_slice_offsetted<T: 'static>(&self, data: &[T], offset: usize) -> VkResult<()> {
                let data_bytes = data.as_ptr() as *const u8;
                let len = data.len() * std::mem::size_of::<T>();
                let bytes = unsafe { std::slice::from_raw_parts(data_bytes, len) };

                self.write_bytes_offsetted(bytes, offset)
        }

        #[allow(dead_code)]
        pub fn write_bytes(&self, bytes: &[u8]) -> VkResult<()> {
                self.write_bytes_offsetted(bytes, 0)
        }

        #[allow(dead_code)]
        pub fn write_bytes_offsetted(&self, bytes: &[u8], offset: usize) -> VkResult<()> {
                assert!(
                        offset + bytes.len() <= self.ainfo.size(),
                        "Tried to write {} bytes with offset {} (total: {}) into buffer of size {}!",
                        bytes.len(),
                        offset,
                        offset + bytes.len(),
                        self.ainfo.size()
                );

                let map = self.map_memory()?;
                unsafe {
                        let dst = map.offset(offset as isize);
                        std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
                }
                Ok(())
        }

        pub fn map_memory(&self) -> VkResult<*mut u8> {
                if self.memory.get().is_null() {
                        self.memory.set(unsafe { self.allocator.map_memory(self.alloc)? });
                }

                Ok(self.memory.get())
        }

        pub fn unmap_memory(&self) {
                self.memory.set(std::ptr::null_mut());
                unsafe {
                        self.allocator.unmap_memory(self.alloc);
                }
        }

        #[allow(dead_code)]
        pub fn flush_all_memory(&self) -> VkResult<()> {
                unsafe { self.allocator.flush_allocation(self.alloc, 0, self.ainfo.size()) }
        }
}

impl_destroyable_expr!(VkBuffer, vk::Buffer, |s: &VkBuffer| {
        if !s.memory.get().is_null() {
                s.unmap_memory();
        }

        unsafe {
                s.allocator.destroy_buffer(s.handle, s.alloc);
        }
});

pub struct VkDynamicUniformBuffer<T: 'static> {
        capacity: usize,
        buffer: VkBuffer,
        element_padded_size: usize,
        _element_type: PhantomData<T>,
}

impl<T: 'static> VkDynamicUniformBuffer<T> {
        pub fn new(
                pdevice: &VkPhysicalDevice,
                device: &ash::Device,
                allocator: Rc<VmaAllocator>,
                capacity: usize,
        ) -> AnyResult<Self> {
                let element_padded_size = pdevice.padded_size_of::<T>();
                let buffer_size = (element_padded_size * capacity) as vk::DeviceSize;
                let buffer = VkBuffer::new_uniform_buffer(&device, allocator, buffer_size)?;

                Ok(Self {
                        buffer,
                        capacity,
                        element_padded_size,
                        _element_type: PhantomData,
                })
        }

        pub fn element_padded_size(&self) -> usize {
                self.element_padded_size
        }

        // Writes data to buffer with the specified index. Returns the element's offset.
        pub fn write(&self, value: &T, index: usize) -> VkResult<usize> {
                assert!(
                        index < self.capacity,
                        "Write to buffer with capacity {} invalid with index {}",
                        self.capacity,
                        index
                );

                let offset = self.element_padded_size * index;
                self.buffer.write_offsetted(value, offset)?;
                Ok(offset)
        }

        pub unsafe fn destroy(&self) {
                self.buffer.destroy();
        }
}

impl<T: 'static> Deref for VkDynamicUniformBuffer<T> {
        type Target = vk::Buffer;

        fn deref(&self) -> &Self::Target {
                &*self.buffer
        }
}
