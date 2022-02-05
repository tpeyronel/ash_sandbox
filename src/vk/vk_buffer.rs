use std::{ops::Deref, rc::Rc};

use ash::vk;
use log::trace;

use crate::AnyResult;

use super::vk_command_buffer::VkReusableCommandBuffer;

#[derive(Clone)]
pub struct VkBufferCreateInfo<'a> {
        pub device: &'a ash::Device,
        pub allocator: Rc<vma::Allocator>,

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
        pub allocator: Rc<vma::Allocator>,

        pub cmd_buffer: &'a VkReusableCommandBuffer,
        pub transfer_queue: vk::Queue,
        pub buffer_usage: vk::BufferUsageFlags,
        pub data: BufferData<'a, T>,
}

pub struct VkBuffer {
        allocator: Rc<vma::Allocator>,

        handle: vk::Buffer,
        alloc: vma::Allocation,
        ainfo: vma::AllocationInfo,

        memory: *mut u8,
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
                };

                let (handle, alloc, ainfo) = create_info.allocator.create_buffer(&handle_cinfo, &alloc_cinfo)?;

                Ok(Self {
                        allocator: create_info.allocator,
                        handle,
                        alloc,
                        ainfo,
                        memory: std::ptr::null_mut(),
                })
        }

        pub fn new_immutable<T>(create_info: VkImmutableBufferCreateInfo<T>) -> AnyResult<Self> {
                let (buffer_data, buffer_size) = match create_info.data {
                        BufferData::FullSlice(s) => (
                                s.as_ptr() as *const u8,
                                (std::mem::size_of::<T>() * s.len()) as vk::DeviceSize,
                        ),
                        BufferData::OffsetLength { data, offset, length } => (
                                unsafe { (data.as_ptr() as *const u8).offset(offset as isize) },
                                length as vk::DeviceSize,
                        ),
                };

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

                let mut staging_buffer = VkBuffer::new(staging_buffer_cinfo)?;

                let map = staging_buffer.map_memory()?;
                unsafe {
                        std::ptr::copy_nonoverlapping(buffer_data, map, buffer_size as usize);
                }
                staging_buffer.unmap_memory()?;

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
                        create_info.cmd_buffer.wait(create_info.device, u64::MAX)?;
                }

                Ok(buffer)
        }

        pub fn new_transfer_src(
                device: &ash::Device,
                allocator: Rc<vma::Allocator>,
                data: &[u8],
        ) -> AnyResult<VkBuffer> {
                let mut staging_buffer = {
                        let buffer_cinfo = VkBufferCreateInfo {
                                device,
                                allocator,
                                buffer_size: data.len() as vk::DeviceSize,
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

                let buffer_data = staging_buffer.map_memory()?;
                unsafe {
                        std::ptr::copy_nonoverlapping(data.as_ptr(), buffer_data, data.len());
                }
                staging_buffer.unmap_memory()?;

                Ok(staging_buffer)
        }

        pub fn write<T: 'static>(&mut self, value: &T) -> vma::Result<()> {
                self.write_offsetted(value, 0)
        }

        pub fn write_offsetted<T: 'static>(&mut self, value: &T, offset: usize) -> vma::Result<()> {
                let map = self.map_memory()?;
                unsafe {
                        let src = value as *const _ as *const u8;
                        let dst = map.offset(offset as isize);
                        std::ptr::copy_nonoverlapping(src, dst, std::mem::size_of::<T>());
                }
                Ok(())
        }

        pub fn map_memory(&mut self) -> vma::Result<*mut u8> {
                if self.memory.is_null() {
                        self.memory = self.allocator.map_memory(&self.alloc)?;
                }

                Ok(self.memory)
        }

        pub fn unmap_memory(&mut self) -> vma::Result<()> {
                self.memory = std::ptr::null_mut();
                self.allocator.unmap_memory(&self.alloc)
        }

        #[allow(dead_code)]
        pub fn flush_all_memory(&self) -> vma::Result<()> {
                self.allocator.flush_allocation(&self.alloc, 0, self.ainfo.get_size())
        }
}

impl Deref for VkBuffer {
        type Target = vk::Buffer;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkBuffer {
        fn drop(&mut self) {
                trace!("Destroying VkBuffer...");
                assert_ne!(self.handle, vk::Buffer::null());

                if !self.memory.is_null() {
                        let _ = self.unmap_memory();
                }

                let _ = self.allocator.destroy_buffer(self.handle, &self.alloc);

                self.handle = vk::Buffer::null();
        }
}
