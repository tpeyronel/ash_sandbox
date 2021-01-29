use std::{error::Error, ops::Deref, sync::Arc};

use ash::{version::DeviceV1_0, vk};
use log::trace;

use crate::{
        image::Image2D,
        vk_buffer::{VkBuffer, VkBufferCreateInfo},
        vk_command_buffer::VkReusableCommandBuffer,
        vk_wrapper::{VkDevice, VkImageView, VkSampler},
        vkma_error::VkmaResult,
};

pub struct VkImageCreateInfo {
        pub image_type:           vk::ImageType,
        pub format:               vk::Format,
        pub extent:               vk::Extent3D,
        pub mip_levels:           u32,
        pub array_layers:         u32,
        pub samples:              vk::SampleCountFlags,
        pub tiling:               vk::ImageTiling,
        pub usage:                vk::ImageUsageFlags,
        pub queue_family_indices: Option<Vec<u32>>,
        pub initial_layout:       vk::ImageLayout,

        pub mem_usage:       vma::MemoryUsage,
        pub alloc_cflags:    vma::AllocationCreateFlags,
        pub required_flags:  vk::MemoryPropertyFlags,
        pub preferred_flags: vk::MemoryPropertyFlags,
}

pub struct VkImageCreateFromImage2DInfo<'a> {
        pub image:               &'a Image2D,
        pub format:              vk::Format,
        pub mip_levels:          u32,
        pub samples:             vk::SampleCountFlags,
        pub transfer_cmd_buffer: &'a VkReusableCommandBuffer,
        pub transfer_queue:      vk::Queue,
}

pub struct VkImage {
        allocator: Arc<vma::Allocator>,

        handle: vk::Image,
        alloc:  vma::Allocation,
        ainfo:  vma::AllocationInfo,
}

impl VkImage {
        pub unsafe fn new(allocator: &Arc<vma::Allocator>, create_info: &VkImageCreateInfo) -> VkmaResult<Self> {
                let (handle, alloc, ainfo) = {
                        let mut vk_img_cinfo = vk::ImageCreateInfo {
                                image_type: create_info.image_type,
                                format: create_info.format,
                                extent: create_info.extent,
                                mip_levels: create_info.mip_levels,
                                array_layers: create_info.array_layers,
                                samples: create_info.samples,
                                tiling: create_info.tiling,
                                usage: create_info.usage,
                                initial_layout: create_info.initial_layout,
                                ..vk::ImageCreateInfo::default()
                        };

                        match &create_info.queue_family_indices {
                                Some(queue_families_indices) => {
                                        assert!(!queue_families_indices.is_empty());

                                        vk_img_cinfo.sharing_mode = vk::SharingMode::CONCURRENT;
                                        vk_img_cinfo.p_queue_family_indices = queue_families_indices.as_ptr();
                                        vk_img_cinfo.queue_family_index_count = queue_families_indices.len() as u32;
                                },
                                None => {
                                        vk_img_cinfo.sharing_mode = vk::SharingMode::EXCLUSIVE;
                                        vk_img_cinfo.p_queue_family_indices = std::ptr::null();
                                        vk_img_cinfo.queue_family_index_count = 0;
                                },
                        }

                        let alloc_cinfo = vma::AllocationCreateInfo {
                                usage:            create_info.mem_usage,
                                flags:            create_info.alloc_cflags,
                                required_flags:   create_info.required_flags,
                                preferred_flags:  create_info.preferred_flags,
                                memory_type_bits: 0,
                                pool:             None,
                                user_data:        None,
                        };

                        allocator.create_image(&vk_img_cinfo, &alloc_cinfo)?
                };

                Ok(Self {
                        allocator: Arc::clone(allocator),
                        handle,
                        alloc,
                        ainfo,
                })
        }

        pub fn from_image_2d(
                device: &Arc<VkDevice>,
                allocator: &Arc<vma::Allocator>,
                create_info: &VkImageCreateFromImage2DInfo,
        ) -> Result<Self, Box<dyn Error>> {
                let staging_buffer = {
                        let buffer_cinfo = VkBufferCreateInfo {
                                device,
                                allocator,
                                buffer_size: create_info.image.data_size() as vk::DeviceSize,
                                buffer_usage: vk::BufferUsageFlags::TRANSFER_SRC,
                                mem_usage: vma::MemoryUsage::CpuToGpu,
                                alloc_flags: vma::AllocationCreateFlags::NONE,
                                req_mem_flags: vk::MemoryPropertyFlags::HOST_VISIBLE,
                                pref_mem_flags: Default::default(),
                                mem_type_bits: 0,
                                q_family_indices: None,
                        };

                        VkBuffer::new(&buffer_cinfo)?
                };

                let buffer_data = staging_buffer.map_memory(&allocator)?;
                unsafe {
                        std::ptr::copy_nonoverlapping(
                                create_info.image.data(),
                                buffer_data,
                                create_info.image.data_size(),
                        );
                }
                staging_buffer.unmap_memory(&allocator)?;
                staging_buffer.flush_memory(&allocator)?;

                let vk_img_cinfo = VkImageCreateInfo {
                        image_type:           vk::ImageType::TYPE_2D,
                        format:               vk::Format::R8G8B8A8_SRGB,
                        extent:               vk::Extent3D {
                                width:  create_info.image.width(),
                                height: create_info.image.height(),
                                depth:  1,
                        },
                        mip_levels:           1,
                        array_layers:         1,
                        samples:              vk::SampleCountFlags::TYPE_1,
                        tiling:               vk::ImageTiling::OPTIMAL,
                        usage:                vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
                        queue_family_indices: None,
                        initial_layout:       vk::ImageLayout::UNDEFINED,
                        mem_usage:            vma::MemoryUsage::GpuOnly,
                        alloc_cflags:         vma::AllocationCreateFlags::DEDICATED_MEMORY,
                        required_flags:       vk::MemoryPropertyFlags::DEVICE_LOCAL,
                        preferred_flags:      Default::default(),
                };

                let vk_img = unsafe { VkImage::new(allocator, &vk_img_cinfo)? };

                create_info.transfer_cmd_buffer.record_and_submit(
                        &device,
                        create_info.transfer_queue,
                        &[],
                        &[],
                        &[],
                        |device, cmd_buffer| {
                                let barrier = vk::ImageMemoryBarrier {
                                        src_access_mask: vk::AccessFlags::empty(),
                                        dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                                        old_layout: vk::ImageLayout::UNDEFINED,
                                        new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                        src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                                        dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                                        image: *vk_img,
                                        subresource_range: vk::ImageSubresourceRange {
                                                aspect_mask:      vk::ImageAspectFlags::COLOR,
                                                base_mip_level:   0,
                                                level_count:      1,
                                                base_array_layer: 0,
                                                layer_count:      1,
                                        },
                                        ..vk::ImageMemoryBarrier::default()
                                };

                                unsafe {
                                        device.cmd_pipeline_barrier(
                                                cmd_buffer,
                                                vk::PipelineStageFlags::TOP_OF_PIPE,
                                                vk::PipelineStageFlags::TRANSFER,
                                                vk::DependencyFlags::empty(),
                                                &[],
                                                &[],
                                                &[barrier],
                                        )
                                };

                                let region = vk::BufferImageCopy {
                                        buffer_offset:       0,
                                        buffer_row_length:   0,
                                        buffer_image_height: 0,
                                        image_subresource:   vk::ImageSubresourceLayers {
                                                aspect_mask:      vk::ImageAspectFlags::COLOR,
                                                mip_level:        0,
                                                base_array_layer: 0,
                                                layer_count:      1,
                                        },
                                        image_offset:        vk::Offset3D {
                                                x: 0, y: 0, z: 0
                                        },
                                        image_extent:        vk::Extent3D {
                                                width:  create_info.image.width(),
                                                height: create_info.image.height(),
                                                depth:  1,
                                        },
                                };

                                unsafe {
                                        device.cmd_copy_buffer_to_image(
                                                cmd_buffer,
                                                *staging_buffer,
                                                *vk_img,
                                                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                                &[region],
                                        )
                                };

                                let barrier = vk::ImageMemoryBarrier {
                                        src_access_mask: vk::AccessFlags::TRANSFER_WRITE,
                                        dst_access_mask: vk::AccessFlags::SHADER_READ,
                                        old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                        new_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                                        src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                                        dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                                        image: *vk_img,
                                        subresource_range: vk::ImageSubresourceRange {
                                                aspect_mask:      vk::ImageAspectFlags::COLOR,
                                                base_mip_level:   0,
                                                level_count:      1,
                                                base_array_layer: 0,
                                                layer_count:      1,
                                        },
                                        ..vk::ImageMemoryBarrier::default()
                                };

                                unsafe {
                                        device.cmd_pipeline_barrier(
                                                cmd_buffer,
                                                vk::PipelineStageFlags::TRANSFER,
                                                vk::PipelineStageFlags::FRAGMENT_SHADER,
                                                vk::DependencyFlags::empty(),
                                                &[],
                                                &[],
                                                &[barrier],
                                        )
                                };

                                Ok(())
                        },
                )?;

                create_info.transfer_cmd_buffer.wait(device, u64::MAX)?;

                Ok(vk_img)
        }
}

impl Deref for VkImage {
        type Target = vk::Image;

        fn deref(&self) -> &Self::Target {
                &self.handle
        }
}

impl Drop for VkImage {
        fn drop(&mut self) {
                trace!("Destroying VkImage...");

                assert_ne!(self.handle, vk::Image::null());

                let _ = self.allocator.destroy_image(self.handle, &self.alloc);

                self.handle = vk::Image::null();
        }
}
