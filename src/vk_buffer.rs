use std::{error::Error, ops::Deref};

use ash::{version::DeviceV1_0, vk};

use crate::vk_context::VkReusableCommandBuffer;

pub struct VkBuffer {
        pub handle: vk::Buffer,
        pub alloc:  vma::Allocation,
        pub ainfo:  vma::AllocationInfo,
}

pub struct VkBufferCreateInfo<'a> {
        pub device:           &'a ash::Device,
        pub allocator:        &'a vma::Allocator,
        pub buffer_size:      vk::DeviceSize,
        pub buffer_usage:     vk::BufferUsageFlags,
        pub mem_usage:        vma::MemoryUsage,
        pub alloc_flags:      vma::AllocationCreateFlags,
        pub req_mem_flags:    vk::MemoryPropertyFlags,
        pub pref_mem_flags:   vk::MemoryPropertyFlags,
        pub mem_type_bits:    u32,
        pub q_family_indices: Option<&'a [u32]>,
}

pub struct VkImmutableBufferCreateInfo<'a, T> {
        pub device:         &'a ash::Device,
        pub allocator:      &'a vma::Allocator,
        pub cmd_buffer:     &'a VkReusableCommandBuffer,
        pub transfer_queue: vk::Queue,
        pub buffer_usage:   vk::BufferUsageFlags,
        pub data:           &'a [T],
}

impl VkBuffer {
        pub fn new(create_info: &VkBufferCreateInfo) -> vma::Result<Self> {
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
                        usage:            create_info.mem_usage,
                        flags:            create_info.alloc_flags,
                        required_flags:   create_info.req_mem_flags,
                        preferred_flags:  create_info.pref_mem_flags,
                        memory_type_bits: create_info.mem_type_bits,
                        pool:             None,
                        user_data:        None,
                };

                let (handle, alloc, ainfo) = create_info.allocator.create_buffer(&handle_cinfo, &alloc_cinfo)?;

                Ok(Self {
                        handle,
                        alloc,
                        ainfo,
                })
        }

        pub fn new_immutable<T>(create_info: &VkImmutableBufferCreateInfo<T>) -> Result<Self, Box<dyn Error>> {
                let buffer_size = (std::mem::size_of::<T>() * create_info.data.len()) as vk::DeviceSize;

                let staging_buffer_cinfo = VkBufferCreateInfo {
                        device: create_info.device,
                        allocator: create_info.allocator,
                        buffer_size,
                        buffer_usage: vk::BufferUsageFlags::TRANSFER_SRC,
                        mem_usage: vma::MemoryUsage::CpuToGpu,
                        alloc_flags: vma::AllocationCreateFlags::NONE,
                        req_mem_flags: vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                        pref_mem_flags: Default::default(),
                        mem_type_bits: 0,
                        q_family_indices: None,
                };

                let staging_buffer = VkBuffer::new(&staging_buffer_cinfo)?;

                let map = staging_buffer.map_memory(create_info.allocator)?;
                unsafe {
                        std::ptr::copy_nonoverlapping(
                                create_info.data.as_ptr() as *const u8,
                                map,
                                buffer_size as usize,
                        );
                }
                staging_buffer.unmap_memory(create_info.allocator)?;

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
                let buffer = VkBuffer::new(&buffer_cinfo)?;

                create_info.cmd_buffer.record_and_submit(
                        create_info.device,
                        create_info.transfer_queue,
                        &[],
                        &[],
                        &[],
                        |device, cmd_buffer| unsafe {
                                let regions = [vk::BufferCopy {
                                        src_offset: 0,
                                        dst_offset: 0,
                                        size:       buffer_size,
                                }];

                                device.cmd_copy_buffer(cmd_buffer, staging_buffer.handle, buffer.handle, &regions);

                                Ok(())
                        },
                )?;

                create_info.cmd_buffer.wait(create_info.device, u64::MAX)?;

                staging_buffer.destroy(&create_info.allocator);

                Ok(Self {
                        handle: buffer.handle,
                        alloc:  buffer.alloc,
                        ainfo:  buffer.ainfo,
                })
        }

        pub fn map_memory(&self, allocator: &vma::Allocator) -> vma::Result<*mut u8> {
                allocator.map_memory(&self.alloc)
        }

        pub fn unmap_memory(&self, allocator: &vma::Allocator) -> vma::Result<()> {
                allocator.unmap_memory(&self.alloc)
        }

        pub fn flush_memory(&self, allocator: &vma::Allocator) -> vma::Result<()> {
                allocator.flush_allocation(&self.alloc, 0, self.ainfo.get_size())
        }

        pub fn destroy(&self, allocator: &vma::Allocator) {
                let _ = allocator.destroy_buffer(self.handle, &self.alloc);
        }
}




impl Deref for VkBuffer {
        type Target = vk::Buffer;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}
