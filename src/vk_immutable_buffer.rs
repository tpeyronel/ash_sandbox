use std::error::Error;

use ash::{
        version::{DeviceV1_0, InstanceV1_0},
        vk,
};

use crate::vk_context::VkReusableCommandBuffer;

pub struct VkImmutableBuffer {
        pub handle: vk::Buffer,
        alloc:      vma::Allocation,
        ainfo:      vma::AllocationInfo,
}

impl VkImmutableBuffer {
        pub fn from_slice<T>(
                device: &ash::Device,
                allocator: &vma::Allocator,
                cmd_buffer: &VkReusableCommandBuffer,
                queue_family_i: u32,
                queue: vk::Queue,
                buffer_usage: vk::BufferUsageFlags,
                data: &[T],
        ) -> Result<Self, Box<dyn Error>> {
                let buffer_size = (std::mem::size_of::<T>() * data.len()) as vk::DeviceSize;

                let staging_buffer_cinfo = vk::BufferCreateInfo::builder()
                        .size(buffer_size)
                        .usage(vk::BufferUsageFlags::TRANSFER_SRC)
                        .sharing_mode(vk::SharingMode::EXCLUSIVE)
                        .queue_family_indices(unsafe { std::slice::from_raw_parts(&queue_family_i as *const _, 1) });

                let staging_alloc_cinfo = vma::AllocationCreateInfo {
                        usage:            vma::MemoryUsage::CpuToGpu,
                        flags:            vma::AllocationCreateFlags::NONE,
                        required_flags:   vk::MemoryPropertyFlags::HOST_VISIBLE,
                        preferred_flags:  vk::MemoryPropertyFlags::empty(),
                        memory_type_bits: 0,
                        pool:             None,
                        user_data:        None,
                };

                let (staging_buffer, staging_alloc, _) =
                        allocator.create_buffer(&staging_buffer_cinfo, &staging_alloc_cinfo)?;

                let map = allocator.map_memory(&staging_alloc)?;

                unsafe {
                        std::ptr::copy_nonoverlapping(data.as_ptr() as *const u8, map, buffer_size as usize);
                }

                allocator.unmap_memory(&staging_alloc)?;
                allocator.flush_allocation(&staging_alloc, 0, buffer_size as usize)?;




                let buffer_cinfo = vk::BufferCreateInfo::builder()
                        .size(buffer_size)
                        .usage(vk::BufferUsageFlags::TRANSFER_DST | buffer_usage)
                        .sharing_mode(vk::SharingMode::EXCLUSIVE)
                        .queue_family_indices(unsafe { std::slice::from_raw_parts(&queue_family_i as *const _, 1) });

                let alloc_cinfo = vma::AllocationCreateInfo {
                        usage:            vma::MemoryUsage::GpuOnly,
                        flags:            vma::AllocationCreateFlags::NONE,
                        required_flags:   vk::MemoryPropertyFlags::DEVICE_LOCAL,
                        preferred_flags:  vk::MemoryPropertyFlags::empty(),
                        memory_type_bits: 0,
                        pool:             None,
                        user_data:        None,
                };

                let (handle, alloc, ainfo) = allocator.create_buffer(&buffer_cinfo, &alloc_cinfo)?;

                cmd_buffer.record_and_submit(&device, queue, &[], &[], &[], |device, cmd_buffer| {
                        let regions = [vk::BufferCopy {
                                src_offset: 0,
                                dst_offset: 0,
                                size:       buffer_size,
                        }];

                        unsafe {
                                device.cmd_copy_buffer(cmd_buffer, staging_buffer, handle, &regions);
                        }
                })?;

                cmd_buffer.wait(device, u64::MAX);

                allocator.destroy_buffer(staging_buffer, &staging_alloc)?;

                Ok(Self {
                        handle,
                        alloc,
                        ainfo,
                })
        }

        pub fn destroy(&self, allocator: &vma::Allocator) -> vma::Result<()> {
                allocator.destroy_buffer(self.handle, &self.alloc)
        }
}
