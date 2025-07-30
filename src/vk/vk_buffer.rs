use std::{cell::Cell, ops::Deref, rc::Rc};

use ash::{prelude::VkResult, vk};
#[allow(unused_imports)]
use log::trace;
use vk_mem::Alloc;

use crate::{
        vk::{vk_context::VkContext, vk_wrapper::HasVkHandle},
        AnyResult,
};

use super::{
        vk_command_buffer::VkReusableCommandBuffer,
        vk_wrapper::{impl_destroyable_deref, impl_destroyable_drop, impl_destroyable_expr, VmaAllocator},
};

#[derive(Clone)]
pub struct VkBufferCreateInfo<'a> {
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
        pub transfer_queue: vk::Queue,
        pub buffer_usage: vk::BufferUsageFlags,
        pub data: BufferData<'a, T>,
}

pub struct VkBuffer {
        allocator: Rc<VmaAllocator>,

        handle: vk::Buffer,
        alloc: vma::Allocation,
        destroyed: Cell<bool>,

        size_in_bytes: vk::DeviceSize,
        memory: Cell<*mut u8>,
}

impl VkBuffer {
        pub fn new(context: &VkContext, create_info: VkBufferCreateInfo) -> VkResult<Self> {
                let mut handle_cinfo = vk::BufferCreateInfo::default()
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
                        priority: 0.0,
                        ..Default::default()
                };

                let (handle, alloc) = unsafe { context.allocator.create_buffer(&handle_cinfo, &alloc_cinfo)? };

                Ok(Self {
                        allocator: Rc::clone(&context.allocator),
                        handle,
                        alloc,
                        destroyed: Cell::new(false),
                        size_in_bytes: create_info.buffer_size,
                        memory: Cell::new(std::ptr::null_mut()),
                })
        }

        pub fn new_immutable<T>(
                context: &VkContext,
                cmd_buffer: &VkReusableCommandBuffer,
                create_info: VkImmutableBufferCreateInfo<T>,
        ) -> AnyResult<Self> {
                let buffer_data = match create_info.data {
                        BufferData::FullSlice(s) => unsafe {
                                std::slice::from_raw_parts(s.as_ptr() as *const u8, s.len() * std::mem::size_of::<T>())
                        },
                        BufferData::OffsetLength { data, offset, length } => unsafe {
                                assert!((offset + length) <= (data.len() * std::mem::size_of::<T>()));

                                std::slice::from_raw_parts((data.as_ptr() as *const u8).add(offset), length)
                        },
                };
                let buffer_size = buffer_data.len() as vk::DeviceSize;

                let staging_buffer_cinfo = VkBufferCreateInfo {
                        buffer_size,
                        buffer_usage: vk::BufferUsageFlags::TRANSFER_SRC,
                        mem_usage: vma::MemoryUsage::CpuOnly,
                        alloc_flags: vma::AllocationCreateFlags::empty(),
                        req_mem_flags: vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                        pref_mem_flags: Default::default(),
                        mem_type_bits: 0,
                        q_family_indices: None,
                };

                let staging_buffer = VkBuffer::new(context, staging_buffer_cinfo)?;

                staging_buffer.write_bytes(buffer_data)?;
                staging_buffer.unmap_memory();

                let buffer_cinfo = VkBufferCreateInfo {
                        buffer_size,
                        buffer_usage: vk::BufferUsageFlags::TRANSFER_DST | create_info.buffer_usage,
                        mem_usage: vma::MemoryUsage::GpuOnly,
                        alloc_flags: vma::AllocationCreateFlags::empty(),
                        req_mem_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                        pref_mem_flags: Default::default(),
                        mem_type_bits: 0,
                        q_family_indices: None,
                };
                let buffer = VkBuffer::new(context, buffer_cinfo)?;

                cmd_buffer.record_and_submit(
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
                        cmd_buffer.wait(u64::MAX)?;
                        staging_buffer.destroy();
                }

                Ok(buffer)
        }

        pub fn new_uniform_buffer(context: &VkContext, buffer_size: vk::DeviceSize) -> VkResult<Self> {
                let cinfo = VkBufferCreateInfo {
                        buffer_size,
                        buffer_usage: vk::BufferUsageFlags::UNIFORM_BUFFER,
                        mem_usage: vma::MemoryUsage::CpuToGpu,
                        alloc_flags: vma::AllocationCreateFlags::empty(),
                        req_mem_flags: vk::MemoryPropertyFlags::HOST_COHERENT | vk::MemoryPropertyFlags::HOST_VISIBLE,
                        pref_mem_flags: Default::default(),
                        mem_type_bits: 0,
                        q_family_indices: None,
                };

                VkBuffer::new(context, cinfo)
        }

        pub fn new_transfer_src(context: &VkContext, buffer_size: vk::DeviceSize) -> VkResult<VkBuffer> {
                let staging_buffer = {
                        let buffer_cinfo = VkBufferCreateInfo {
                                buffer_size,
                                buffer_usage: vk::BufferUsageFlags::TRANSFER_SRC,
                                mem_usage: vma::MemoryUsage::CpuOnly,
                                alloc_flags: vma::AllocationCreateFlags::empty(),
                                req_mem_flags: vk::MemoryPropertyFlags::HOST_VISIBLE
                                        | vk::MemoryPropertyFlags::HOST_COHERENT,
                                pref_mem_flags: Default::default(),
                                mem_type_bits: 0,
                                q_family_indices: None,
                        };

                        VkBuffer::new(context, buffer_cinfo)?
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
                        offset + bytes.len() <= self.size_in_bytes as usize,
                        "Tried to write {} bytes with offset {} (total: {}) into buffer of size {}!",
                        bytes.len(),
                        offset,
                        offset + bytes.len(),
                        self.size_in_bytes
                );

                let map = self.map_memory()?;
                unsafe {
                        let src = bytes.as_ptr();
                        let dst = map.add(offset);
                        std::ptr::copy_nonoverlapping(src, dst, bytes.len());
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
                unsafe { self.allocator.flush_allocation(self.alloc, 0, self.size_in_bytes) }
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

impl HasVkHandle<vk::Buffer> for &VkBuffer {
        fn handle(self) -> vk::Buffer {
                self.handle
        }
}

pub struct VkDynamicUniformBuffer {
        capacity: usize,
        buffer: VkBuffer,
        element_size: usize,
        element_padded_size: usize,
}

impl VkDynamicUniformBuffer {
        pub fn new(context: &VkContext, element_size: usize, capacity: usize) -> AnyResult<Self> {
                let element_padded_size = context.pdevice.calc_padded_size(element_size);
                let buffer_size = (element_padded_size * capacity) as vk::DeviceSize;
                let buffer = VkBuffer::new_uniform_buffer(context, buffer_size)?;

                Ok(Self {
                        buffer,
                        capacity,
                        element_size,
                        element_padded_size,
                })
        }

        pub fn element_padded_size(&self) -> usize {
                self.element_padded_size
        }

        // Writes data to buffer with the specified index. Returns the element's offset.
        pub fn write<T: 'static>(&self, value: &T, index: usize) -> VkResult<usize> {
                assert!(
                        index < self.capacity,
                        "Write to buffer with capacity {} invalid with index {}",
                        self.capacity,
                        index
                );

                assert_eq!(self.element_size, std::mem::size_of::<T>());

                let offset = self.element_padded_size * index;
                self.buffer.write_offsetted(value, offset)?;
                Ok(offset)
        }

        pub unsafe fn destroy(&self) {
                self.buffer.destroy();
        }
}

impl Deref for VkDynamicUniformBuffer {
        type Target = vk::Buffer;

        fn deref(&self) -> &Self::Target {
                &*self.buffer
        }
}
