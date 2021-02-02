use std::{error::Error, ops::Deref, sync::Arc};

use ash::{
        version::{DeviceV1_0, InstanceV1_0},
        vk,
};
use log::{debug, error, info, trace, warn};

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
        pub image:            &'a Image2D,
        pub format:           vk::Format,
        pub mip_levels:       u32,
        pub samples:          vk::SampleCountFlags,
        pub setup_cmd_buffer: &'a VkReusableCommandBuffer,
        pub graphics_queue:   vk::Queue,
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
                instance: &ash::Instance,
                pdevice: vk::PhysicalDevice,
                device: &Arc<VkDevice>,
                allocator: &Arc<vma::Allocator>,
                cinfo: &VkImageCreateFromImage2DInfo,
        ) -> Result<Self, Box<dyn Error>> {
                let staging_buffer =
                        Self::create_staging_buffer(device, allocator, cinfo.image.data(), cinfo.image.data_bsize())?;

                let vk_img_cinfo = VkImageCreateInfo {
                        image_type:           vk::ImageType::TYPE_2D,
                        format:               vk::Format::R8G8B8A8_SRGB,
                        extent:               vk::Extent3D {
                                width:  cinfo.image.width(),
                                height: cinfo.image.height(),
                                depth:  1,
                        },
                        mip_levels:           cinfo.mip_levels,
                        array_layers:         1,
                        samples:              vk::SampleCountFlags::TYPE_1,
                        tiling:               vk::ImageTiling::OPTIMAL,
                        usage:                vk::ImageUsageFlags::TRANSFER_SRC
                                | vk::ImageUsageFlags::TRANSFER_DST
                                | vk::ImageUsageFlags::SAMPLED,
                        queue_family_indices: None,
                        initial_layout:       vk::ImageLayout::UNDEFINED,
                        mem_usage:            vma::MemoryUsage::GpuOnly,
                        alloc_cflags:         vma::AllocationCreateFlags::DEDICATED_MEMORY,
                        required_flags:       vk::MemoryPropertyFlags::DEVICE_LOCAL,
                        preferred_flags:      Default::default(),
                };

                let vk_img = unsafe { VkImage::new(allocator, &vk_img_cinfo)? };


                unsafe {
                        let cmd_buffer = **cinfo.setup_cmd_buffer;

                        cinfo.setup_cmd_buffer.begin(&device)?;

                        Self::cmd_transition_img_layout(&TransitionImageLayoutInfo {
                                device,
                                cmd_buffer,

                                old_layout: vk::ImageLayout::UNDEFINED,
                                new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,

                                image: *vk_img,
                                base_mip_level: 0,
                                mip_level_count: cinfo.mip_levels,
                                aspect_mask: vk::ImageAspectFlags::COLOR,

                                src_access_mask: vk::AccessFlags::empty(),
                                dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,

                                src_stage_mask: vk::PipelineStageFlags::TOP_OF_PIPE,
                                dst_stage_mask: vk::PipelineStageFlags::TRANSFER,
                        });

                        Self::cmd_copy_buffer_to_image(
                                device,
                                cmd_buffer,
                                cinfo.image.width(),
                                cinfo.image.height(),
                                *staging_buffer,
                                *vk_img,
                                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        );

                        Self::cmd_gen_mipmaps(
                                instance,
                                pdevice,
                                device,
                                cmd_buffer,
                                *vk_img,
                                vk::Format::R8G8B8A8_SRGB,
                                cinfo.image.width(),
                                cinfo.image.height(),
                                cinfo.mip_levels,
                        );


                        cinfo.setup_cmd_buffer
                                .end_and_submit(&device, cinfo.graphics_queue, &[], &[], &[])?;

                        cinfo.setup_cmd_buffer.wait(device, u64::MAX)?;
                }


                Ok(vk_img)
        }

        fn create_staging_buffer(
                device: &ash::Device,
                allocator: &Arc<vma::Allocator>,
                data: *const u8,
                data_bsize: usize,
        ) -> VkmaResult<VkBuffer> {
                let staging_buffer = {
                        let buffer_cinfo = VkBufferCreateInfo {
                                device,
                                allocator,

                                buffer_size: data_bsize as vk::DeviceSize,
                                buffer_usage: vk::BufferUsageFlags::TRANSFER_SRC,
                                mem_usage: vma::MemoryUsage::CpuToGpu,
                                alloc_flags: vma::AllocationCreateFlags::NONE,
                                req_mem_flags: vk::MemoryPropertyFlags::HOST_VISIBLE
                                        | vk::MemoryPropertyFlags::HOST_COHERENT,
                                pref_mem_flags: Default::default(),
                                mem_type_bits: 0,
                                q_family_indices: None,
                        };

                        VkBuffer::new(&buffer_cinfo)?
                };

                let buffer_data = staging_buffer.map_memory(&allocator)?;
                unsafe {
                        std::ptr::copy_nonoverlapping(data, buffer_data, data_bsize);
                }
                staging_buffer.unmap_memory(&allocator)?;

                Ok(staging_buffer)
        }

        fn cmd_transition_img_layout(tinfo: &TransitionImageLayoutInfo) {
                let barrier = vk::ImageMemoryBarrier {
                        src_access_mask: tinfo.src_access_mask,
                        dst_access_mask: tinfo.dst_access_mask,
                        old_layout: tinfo.old_layout,
                        new_layout: tinfo.new_layout,
                        src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                        dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                        image: tinfo.image,
                        subresource_range: vk::ImageSubresourceRange {
                                aspect_mask:      tinfo.aspect_mask,
                                base_mip_level:   tinfo.base_mip_level,
                                level_count:      tinfo.mip_level_count,
                                base_array_layer: 0,
                                layer_count:      1,
                        },
                        ..vk::ImageMemoryBarrier::default()
                };

                unsafe {
                        tinfo.device.cmd_pipeline_barrier(
                                tinfo.cmd_buffer,
                                tinfo.src_stage_mask,
                                tinfo.dst_stage_mask,
                                vk::DependencyFlags::empty(),
                                &[],
                                &[],
                                std::slice::from_ref(&barrier),
                        )
                };
        }

        fn cmd_copy_buffer_to_image(
                device: &ash::Device,
                cmd_buffer: vk::CommandBuffer,
                width: u32,
                height: u32,
                src_buffer: vk::Buffer,
                dst_image: vk::Image,
                dst_image_layout: vk::ImageLayout,
        ) {
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
                                width,
                                height,
                                depth: 1,
                        },
                };

                unsafe {
                        device.cmd_copy_buffer_to_image(
                                cmd_buffer,
                                src_buffer,
                                dst_image,
                                dst_image_layout,
                                std::slice::from_ref(&region),
                        )
                };
        }

        fn cmd_gen_mipmaps(
                instance: &ash::Instance,
                pdevice: vk::PhysicalDevice,
                device: &ash::Device,
                cmd_buffer: vk::CommandBuffer,
                image: vk::Image,
                image_format: vk::Format,
                width: u32,
                height: u32,
                mip_levels: u32,
        ) {
                assert!(unsafe {
                        instance.get_physical_device_format_properties(pdevice, image_format)
                                .optimal_tiling_features
                                .contains(vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR)
                });

                let mut tinfo = TransitionImageLayoutInfo {
                        device,
                        cmd_buffer,

                        image,
                        aspect_mask: vk::ImageAspectFlags::COLOR,

                        base_mip_level: Default::default(),
                        mip_level_count: 1,

                        src_access_mask: Default::default(),
                        dst_access_mask: Default::default(),
                        old_layout: Default::default(),
                        new_layout: Default::default(),

                        src_stage_mask: Default::default(),
                        dst_stage_mask: Default::default(),
                };

                let mut prev_mip_width = width;
                let mut prev_mip_height = height;

                for i in 1..mip_levels {
                        let this_mip_width = if prev_mip_width > 1 { prev_mip_width / 2 } else { 1 };
                        let this_mip_height = if prev_mip_height > 1 { prev_mip_height / 2 } else { 1 };

                        tinfo.base_mip_level = i - 1;

                        tinfo.old_layout = vk::ImageLayout::TRANSFER_DST_OPTIMAL;
                        tinfo.new_layout = vk::ImageLayout::TRANSFER_SRC_OPTIMAL;
                        tinfo.src_access_mask = vk::AccessFlags::TRANSFER_WRITE;
                        tinfo.dst_access_mask = vk::AccessFlags::TRANSFER_READ;
                        tinfo.src_stage_mask = vk::PipelineStageFlags::TRANSFER;
                        tinfo.dst_stage_mask = vk::PipelineStageFlags::TRANSFER;

                        Self::cmd_transition_img_layout(&tinfo);


                        let mut blit = vk::ImageBlit::default();

                        blit.src_subresource.aspect_mask = vk::ImageAspectFlags::COLOR;
                        blit.src_subresource.mip_level = i - 1;
                        blit.src_subresource.base_array_layer = 0;
                        blit.src_subresource.layer_count = 1;
                        blit.src_offsets[0].x = 0;
                        blit.src_offsets[0].y = 0;
                        blit.src_offsets[0].z = 0;
                        blit.src_offsets[1].x = prev_mip_width as i32;
                        blit.src_offsets[1].y = prev_mip_height as i32;
                        blit.src_offsets[1].z = 1;

                        blit.dst_subresource.aspect_mask = vk::ImageAspectFlags::COLOR;
                        blit.dst_subresource.mip_level = i;
                        blit.dst_subresource.base_array_layer = 0;
                        blit.dst_subresource.layer_count = 1;
                        blit.dst_offsets[0].x = 0;
                        blit.dst_offsets[0].y = 0;
                        blit.dst_offsets[0].z = 0;
                        blit.dst_offsets[1].x = this_mip_width as i32;
                        blit.dst_offsets[1].y = this_mip_height as i32;
                        blit.dst_offsets[1].z = 1;

                        unsafe {
                                device.cmd_blit_image(
                                        cmd_buffer,
                                        image,
                                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                                        image,
                                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                        std::slice::from_ref(&blit),
                                        vk::Filter::LINEAR,
                                );
                        }


                        tinfo.old_layout = vk::ImageLayout::TRANSFER_SRC_OPTIMAL;
                        tinfo.new_layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
                        tinfo.src_access_mask = vk::AccessFlags::TRANSFER_READ;
                        tinfo.dst_access_mask = vk::AccessFlags::SHADER_READ;
                        tinfo.src_stage_mask = vk::PipelineStageFlags::TRANSFER;
                        tinfo.dst_stage_mask = vk::PipelineStageFlags::FRAGMENT_SHADER;

                        Self::cmd_transition_img_layout(&tinfo);

                        prev_mip_width = this_mip_width;
                        prev_mip_height = this_mip_height;
                }

                tinfo.base_mip_level = mip_levels - 1;
                tinfo.old_layout = vk::ImageLayout::TRANSFER_DST_OPTIMAL;
                tinfo.new_layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
                tinfo.src_access_mask = vk::AccessFlags::TRANSFER_WRITE;
                tinfo.dst_access_mask = vk::AccessFlags::SHADER_READ;
                tinfo.src_stage_mask = vk::PipelineStageFlags::TRANSFER;
                tinfo.dst_stage_mask = vk::PipelineStageFlags::FRAGMENT_SHADER;

                Self::cmd_transition_img_layout(&tinfo);
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





struct TransitionImageLayoutInfo<'a> {
        device:          &'a ash::Device,
        cmd_buffer:      vk::CommandBuffer,
        image:           vk::Image,
        base_mip_level:  u32,
        mip_level_count: u32,
        aspect_mask:     vk::ImageAspectFlags,
        src_access_mask: vk::AccessFlags,
        dst_access_mask: vk::AccessFlags,
        old_layout:      vk::ImageLayout,
        new_layout:      vk::ImageLayout,
        src_stage_mask:  vk::PipelineStageFlags,
        dst_stage_mask:  vk::PipelineStageFlags,
}
