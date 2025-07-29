use std::{cell::Cell, ffi::CString, ops::Deref, rc::Rc};

use ash::{
        prelude::VkResult,
        vk::{self},
};
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use vk_mem::Alloc;

use crate::{
        asset_manager::{Image, ImageFlags},
        util::{RefIntoBytesSlice, RefIntoSlice},
        vk::{vk_image_subresource_range::ImageSubresourceRangeUtil, vk_wrapper::HasVkHandle},
};

use super::{
        vk_buffer::VkBuffer,
        vk_command_buffer::VkReusableCommandBuffer,
        vk_format::VkFormatProperties,
        vk_util::{vk_format_from_image_format_and_color_space, BytesPerPixel},
        vk_wrapper::{
                impl_destroyable_deref, impl_destroyable_drop, impl_destroyable_expr, VkDebugUtils, VkDevice, VkObject,
                VmaAllocator,
        },
};

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum MipLevels {
        Log2,
        N(u32),
}

impl MipLevels {
        pub fn to_value(&self, width: u32, height: u32) -> u32 {
                match *self {
                        MipLevels::Log2 => (u32::max(width, height) as f32).log2().floor() as u32 + 1,
                        MipLevels::N(n) => n,
                }
        }
}

pub struct VkImageCreateInfo {
        pub flags: vk::ImageCreateFlags,
        pub image_type: vk::ImageType,
        pub format: vk::Format,
        pub extent: vk::Extent3D,
        pub mip_levels: u32,
        pub array_layers: u32,
        pub samples: vk::SampleCountFlags,
        pub tiling: vk::ImageTiling,
        pub usage: vk::ImageUsageFlags,
        pub queue_family_indices: Option<Vec<u32>>,
        pub initial_layout: vk::ImageLayout,

        pub mem_usage: vma::MemoryUsage,
        pub alloc_cflags: vma::AllocationCreateFlags,
        pub required_flags: vk::MemoryPropertyFlags,
        pub preferred_flags: vk::MemoryPropertyFlags,
}

pub struct VkImageCreateFromImageInfo<'a> {
        pub image: &'a Image,
        pub mip_levels: MipLevels,
        pub setup_cmd_buffer: &'a VkReusableCommandBuffer,
        pub transfer_queue: vk::Queue,
}

pub struct VkImageCreateFromDataInfo<'a> {
        pub data: &'a [u8],
        pub data_format: vk::Format,
        pub width: u32,
        pub height: u32,
        pub format: vk::Format,
        pub mip_levels: MipLevels,
        pub setup_cmd_buffer: &'a VkReusableCommandBuffer,
        pub transfer_queue: vk::Queue,
}

pub struct VkImageCubemapCreateInfo {
        pub format: vk::Format,
        pub size: u32,
        pub mip_levels: MipLevels,
        pub additional_usage_flags: vk::ImageUsageFlags,
}

#[allow(dead_code)]
pub struct VkImage {
        allocator: Rc<VmaAllocator>,

        handle: vk::Image,
        alloc: vma::Allocation,

        destroyed: Cell<bool>,

        pub format: vk::Format,
        pub width: u32,
        pub height: u32,
        pub depth: u32,
        pub mip_levels: u32, // mip_levels >= 1
        pub array_layers: u32,
}

impl HasVkHandle<vk::Image> for &VkImage {
        fn handle(self) -> vk::Image {
                self.handle
        }
}

impl VkImage {
        pub unsafe fn new(allocator: Rc<VmaAllocator>, create_info: &VkImageCreateInfo) -> VkResult<Self> {
                let (handle, alloc) = {
                        let mut vk_img_cinfo = vk::ImageCreateInfo {
                                flags: create_info.flags,
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
                                Some(queue_family_indices) => {
                                        assert!(!queue_family_indices.is_empty());

                                        vk_img_cinfo.sharing_mode = vk::SharingMode::CONCURRENT;
                                        vk_img_cinfo.p_queue_family_indices = queue_family_indices.as_ptr();
                                        vk_img_cinfo.queue_family_index_count = queue_family_indices.len() as u32;
                                },
                                None => {
                                        vk_img_cinfo.sharing_mode = vk::SharingMode::EXCLUSIVE;
                                        vk_img_cinfo.p_queue_family_indices = std::ptr::null();
                                        vk_img_cinfo.queue_family_index_count = 0;
                                },
                        }

                        let alloc_cinfo = vma::AllocationCreateInfo {
                                usage: create_info.mem_usage,
                                flags: create_info.alloc_cflags,
                                required_flags: create_info.required_flags,
                                preferred_flags: create_info.preferred_flags,
                                memory_type_bits: 0,
                                priority: 0.0,
                                ..Default::default()
                        };

                        allocator.create_image(&vk_img_cinfo, &alloc_cinfo)?
                };

                Ok(Self {
                        allocator,
                        handle,
                        alloc,
                        destroyed: Cell::new(false),
                        format: create_info.format,
                        width: create_info.extent.width,
                        height: create_info.extent.height,
                        depth: create_info.extent.depth,
                        mip_levels: create_info.mip_levels,
                        array_layers: create_info.array_layers,
                })
        }

        pub unsafe fn from_image(
                instance: &ash::Instance,
                pdevice: vk::PhysicalDevice,
                device: &ash::Device,
                allocator: Rc<VmaAllocator>,
                cinfo: &VkImageCreateFromImageInfo,
        ) -> VkResult<Self> {
                let image = cinfo.image;

                let src_format = vk_format_from_image_format_and_color_space(image.format, image.color_space);
                let dst_format = match src_format {
                        vk::Format::R8G8B8_SRGB => vk::Format::R8G8B8A8_SRGB,
                        vk::Format::R8G8B8_UNORM => vk::Format::R8G8B8A8_UNORM,
                        vk::Format::R16G16B16_SFLOAT => vk::Format::R16G16B16A16_SFLOAT,
                        vk::Format::R32G32B32_SFLOAT => vk::Format::R32G32B32A32_SFLOAT,
                        f => f,
                };

                let mip_levels = cinfo.mip_levels.to_value(image.width, image.height);
                let mips_to_copy = if mip_levels != image.mipmaps {
                        if image.mipmaps != 1 {
                                warn!(
                                        "image {:?} has {} precomputed mipmaps but {} were requested, generating all from mipmap 0",
                                        image.name, image.mipmaps, mip_levels,
                                );
                        }
                        1
                } else {
                        mip_levels
                };

                let mut flags = vk::ImageCreateFlags::empty();
                if image.flags.contains(ImageFlags::CUBEMAP) {
                        flags |= vk::ImageCreateFlags::CUBE_COMPATIBLE;
                }

                let vk_image = {
                        let vk_image_cinfo = VkImageCreateInfo {
                                flags,
                                image_type: vk::ImageType::TYPE_2D,
                                format: dst_format,
                                extent: vk::Extent3D {
                                        width: image.width,
                                        height: image.height,
                                        depth: 1,
                                },
                                mip_levels,
                                array_layers: image.layers,
                                samples: vk::SampleCountFlags::TYPE_1,
                                tiling: vk::ImageTiling::OPTIMAL,
                                usage: vk::ImageUsageFlags::TRANSFER_SRC
                                        | vk::ImageUsageFlags::TRANSFER_DST
                                        | vk::ImageUsageFlags::SAMPLED,
                                queue_family_indices: None,
                                initial_layout: vk::ImageLayout::UNDEFINED,
                                mem_usage: vma::MemoryUsage::GpuOnly,
                                alloc_cflags: vma::AllocationCreateFlags::DEDICATED_MEMORY,
                                required_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                                preferred_flags: Default::default(),
                        };

                        VkImage::new(Rc::clone(&allocator), &vk_image_cinfo)?
                };

                let mut deletion_queue = vec![];
                cinfo.setup_cmd_buffer.begin(device)?;
                let cmd_buffer = **cinfo.setup_cmd_buffer;

                Self::cmd_transition_img_layout(
                        device,
                        cmd_buffer,
                        &TransitionImageLayoutInfo {
                                image: *vk_image,
                                old_layout: vk::ImageLayout::UNDEFINED,
                                new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                src_stage_mask: vk::PipelineStageFlags2::NONE,
                                src_access_mask: vk::AccessFlags2::NONE,
                                dst_stage_mask: vk::PipelineStageFlags2::TRANSFER,
                                dst_access_mask: vk::AccessFlags2::TRANSFER_WRITE,
                                subresource_range: vk::ImageSubresourceRange::full_color(),
                        },
                );

                let dst_layer_stride = dst_format.compute_stride_with_mipmaps(image.width, image.height, mip_levels);
                let buffer_size = (image.layers * dst_layer_stride) as vk::DeviceSize;
                let staging_buffer = VkBuffer::new_transfer_src(device, Rc::clone(&allocator), buffer_size)?;

                let mut offset = 0;
                let mut copies = vec![];
                for layer in 0..image.layers {
                        let mut width = image.width;
                        let mut height = image.height;
                        for mipmap in 0..mips_to_copy {
                                let data = image.get_data(layer, mipmap);

                                match (src_format, dst_format) {
                                        (src, dst) if src == dst => {
                                                staging_buffer.write_bytes_offsetted(data, offset)?;
                                        },
                                        (vk::Format::R8G8B8_SRGB, vk::Format::R8G8B8A8_SRGB)
                                        | (vk::Format::R8G8B8_UNORM, vk::Format::R8G8B8A8_UNORM) => {
                                                warn!("slow format: {:?}", src_format);

                                                for (i, rgb) in data.chunks(3).enumerate() {
                                                        staging_buffer.write_bytes_offsetted(
                                                                &[rgb[0], rgb[1], rgb[2], u8::MAX],
                                                                i * 4,
                                                        )?;
                                                }
                                        },
                                        (vk::Format::R32G32B32_SFLOAT, vk::Format::R32G32B32A32_SFLOAT) => {
                                                warn!("slow format: {:?}", src_format);

                                                let onef = 1.0f32;
                                                let onef_bytes = onef.as_bytes();

                                                for (i, rgb) in data.chunks(12).enumerate() {
                                                        let bytes = [rgb, onef_bytes].concat();

                                                        staging_buffer.write_bytes_offsetted(&bytes, i * 16)?;
                                                }
                                        },
                                        _ => panic!(
                                                "unsupported format transcoding: {:?} to {:?}",
                                                src_format, dst_format
                                        ),
                                }

                                copies.push(vk::BufferImageCopy {
                                        buffer_offset: offset as vk::DeviceSize,
                                        buffer_row_length: 0,
                                        buffer_image_height: 0,
                                        image_subresource: vk::ImageSubresourceLayers {
                                                aspect_mask: vk::ImageAspectFlags::COLOR,
                                                mip_level: mipmap,
                                                base_array_layer: layer,
                                                layer_count: 1,
                                        },
                                        image_offset: vk::Offset3D { x: 0, y: 0, z: 0 },
                                        image_extent: vk::Extent3D {
                                                width,
                                                height,
                                                depth: 1,
                                        },
                                });

                                offset += data.len();
                                width = 1.max(width / 2);
                                height = 1.max(height / 2);
                        }
                }
                staging_buffer.unmap_memory();

                device.cmd_copy_buffer_to_image(
                        cmd_buffer,
                        *staging_buffer,
                        *vk_image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &copies,
                );

                deletion_queue.push(VkObject::Buffer(staging_buffer));

                if mips_to_copy != mip_levels {
                        Self::cmd_gen_mipmaps(&GenerateMipmapsInfo {
                                instance,
                                pdevice,
                                device,
                                cmd_buffer,
                                image: *vk_image,
                                image_format: vk_image.format,
                                width: vk_image.width,
                                height: vk_image.height,
                                mip_levels,
                                old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                new_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                                src_stage_mask: vk::PipelineStageFlags2::TRANSFER,
                                src_access_mask: vk::AccessFlags2::TRANSFER_WRITE,
                                dst_stage_mask: vk::PipelineStageFlags2::FRAGMENT_SHADER,
                                dst_access_mask: vk::AccessFlags2::SHADER_READ,
                        });
                } else {
                        Self::cmd_transition_img_layout(
                                device,
                                cmd_buffer,
                                &TransitionImageLayoutInfo {
                                        image: *vk_image,
                                        old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                        new_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                                        src_stage_mask: vk::PipelineStageFlags2::TRANSFER,
                                        src_access_mask: vk::AccessFlags2::TRANSFER_WRITE,
                                        dst_stage_mask: vk::PipelineStageFlags2::FRAGMENT_SHADER,
                                        dst_access_mask: vk::AccessFlags2::SHADER_READ,
                                        subresource_range: vk::ImageSubresourceRange::full_color(),
                                },
                        );
                }

                cinfo.setup_cmd_buffer
                        .end_and_submit(device, cinfo.transfer_queue, &[], &[], &[])?;

                cinfo.setup_cmd_buffer.wait(u64::MAX)?;
                for o in deletion_queue.into_iter().rev() {
                        o.destroy();
                }

                Ok(vk_image)
        }

        pub unsafe fn from_data(
                instance: &ash::Instance,
                pdevice: vk::PhysicalDevice,
                device: &ash::Device,
                allocator: Rc<VmaAllocator>,
                cinfo: &VkImageCreateFromDataInfo,
        ) -> VkResult<Self> {
                let src_format = cinfo.data_format;
                let dst_format = cinfo.format;
                let mip_levels = cinfo.mip_levels.to_value(cinfo.width, cinfo.height);

                let vk_img_cinfo = VkImageCreateInfo {
                        flags: Default::default(),
                        image_type: vk::ImageType::TYPE_2D,
                        format: dst_format,
                        extent: vk::Extent3D {
                                width: cinfo.width,
                                height: cinfo.height,
                                depth: 1,
                        },
                        mip_levels,
                        array_layers: 1,
                        samples: vk::SampleCountFlags::TYPE_1,
                        tiling: vk::ImageTiling::OPTIMAL,
                        usage: vk::ImageUsageFlags::TRANSFER_SRC
                                | vk::ImageUsageFlags::TRANSFER_DST
                                | vk::ImageUsageFlags::SAMPLED,
                        queue_family_indices: None,
                        initial_layout: vk::ImageLayout::UNDEFINED,
                        mem_usage: vma::MemoryUsage::GpuOnly,
                        alloc_cflags: vma::AllocationCreateFlags::DEDICATED_MEMORY,
                        required_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                        preferred_flags: Default::default(),
                };

                let vk_img = VkImage::new(Rc::clone(&allocator), &vk_img_cinfo)?;

                let load_strategy = Self::figure_load_strategy(instance, pdevice, src_format, dst_format);

                let mut deletion_queue = vec![];
                cinfo.setup_cmd_buffer.begin(device)?;
                let cmd_buffer = **cinfo.setup_cmd_buffer;

                Self::cmd_transition_img_layout(
                        device,
                        cmd_buffer,
                        &TransitionImageLayoutInfo {
                                image: *vk_img,
                                old_layout: vk::ImageLayout::UNDEFINED,
                                new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                src_stage_mask: vk::PipelineStageFlags2::NONE,
                                src_access_mask: vk::AccessFlags2::NONE,
                                dst_stage_mask: vk::PipelineStageFlags2::TRANSFER,
                                dst_access_mask: vk::AccessFlags2::TRANSFER_WRITE,
                                subresource_range: vk::ImageSubresourceRange::full_color(),
                        },
                );

                match load_strategy {
                        VkLoadStrategy::StagingBufferDirect | VkLoadStrategy::StagingBufferConvert => {
                                let buffer_size =
                                        (cinfo.width * cinfo.height * dst_format.bytes_per_pixel()) as vk::DeviceSize;
                                let staging_buffer =
                                        VkBuffer::new_transfer_src(device, Rc::clone(&allocator), buffer_size)?;

                                if load_strategy == VkLoadStrategy::StagingBufferDirect {
                                        staging_buffer.write_bytes(cinfo.data)?;
                                } else {
                                        match (src_format, dst_format) {
                                                (vk::Format::R8G8B8_SRGB, vk::Format::R8G8B8A8_SRGB)
                                                | (vk::Format::R8G8B8_UNORM, vk::Format::R8G8B8A8_UNORM) => {
                                                        assert_eq!(cinfo.data.len() % 3, 0);
                                                        warn!("slow format: {:?}", src_format);

                                                        for (i, rgb) in cinfo.data.chunks(3).enumerate() {
                                                                staging_buffer.write_bytes_offsetted(
                                                                        &[rgb[0], rgb[1], rgb[2], u8::MAX],
                                                                        i * 4,
                                                                )?;
                                                        }
                                                },
                                                (vk::Format::R32G32B32_SFLOAT, vk::Format::R32G32B32A32_SFLOAT) => {
                                                        assert_eq!(cinfo.data.len() % 12, 0);
                                                        warn!("slow format: {:?}", src_format);

                                                        let onef = 1.0f32;
                                                        let onef_bytes = onef.as_bytes();

                                                        for (i, rgb) in cinfo.data.chunks(12).enumerate() {
                                                                staging_buffer.write_bytes_offsetted(
                                                                        &[
                                                                                rgb[0],
                                                                                rgb[1],
                                                                                rgb[2],
                                                                                rgb[3],
                                                                                rgb[4],
                                                                                rgb[5],
                                                                                rgb[6],
                                                                                rgb[7],
                                                                                rgb[8],
                                                                                rgb[9],
                                                                                rgb[10],
                                                                                rgb[11],
                                                                                onef_bytes[0],
                                                                                onef_bytes[1],
                                                                                onef_bytes[2],
                                                                                onef_bytes[3],
                                                                        ],
                                                                        i * 16,
                                                                )?;
                                                        }
                                                },
                                                _ => panic!(
                                                        "unsupported (initial, final) vk::Format pair ({:?}, {:?})",
                                                        src_format, dst_format
                                                ),
                                        }
                                }
                                staging_buffer.unmap_memory();

                                Self::cmd_copy_buffer_to_image(
                                        device,
                                        cmd_buffer,
                                        cinfo.width,
                                        cinfo.height,
                                        1,
                                        *staging_buffer,
                                        *vk_img,
                                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                );

                                deletion_queue.push(VkObject::Buffer(staging_buffer));
                        },
                        VkLoadStrategy::LinearImage => {
                                todo!()
                                // let src_vk_img_cinfo = VkImageCreateInfo {
                                //         flags: Default::default(),
                                //         image_type: vk::ImageType::TYPE_2D,
                                //         format: dst_format,
                                //         extent: vk::Extent3D {
                                //                 width: cinfo.width,
                                //                 height: cinfo.height,
                                //                 depth: 1,
                                //         },
                                //         mip_levels,
                                //         array_layers: 1,
                                //         samples: vk::SampleCountFlags::TYPE_1,
                                //         tiling: vk::ImageTiling::LINEAR,
                                //         usage: vk::ImageUsageFlags::TRANSFER_SRC,
                                //         queue_family_indices: None,
                                //         initial_layout: vk::ImageLayout::UNDEFINED,
                                //         mem_usage: vma::MemoryUsage::CpuToGpu,
                                //         alloc_cflags: vma::AllocationCreateFlags::empty(),
                                //         required_flags: vk::MemoryPropertyFlags::empty(),
                                //         preferred_flags: vk::MemoryPropertyFlags::empty(),
                                // };

                                // let src_vk_img = VkImage::new(allocator, &src_vk_img_cinfo)?;

                                // let src_vk_img_map = allocator.map_memory(src_vk_img.alloc)?;
                                // d
                        },
                }

                // This is missing check of mip_levels > 1 i think.
                Self::cmd_gen_mipmaps(&GenerateMipmapsInfo {
                        instance,
                        pdevice,
                        device,
                        cmd_buffer,
                        image: *vk_img,
                        image_format: vk_img.format,
                        width: vk_img.width,
                        height: vk_img.height,
                        mip_levels: vk_img.mip_levels,
                        old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        new_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                        src_stage_mask: vk::PipelineStageFlags2::TRANSFER,
                        src_access_mask: vk::AccessFlags2::TRANSFER_WRITE,
                        dst_stage_mask: vk::PipelineStageFlags2::FRAGMENT_SHADER,
                        dst_access_mask: vk::AccessFlags2::SHADER_READ,
                });

                cinfo.setup_cmd_buffer
                        .end_and_submit(device, cinfo.transfer_queue, &[], &[], &[])?;

                cinfo.setup_cmd_buffer.wait(u64::MAX)?;
                for o in deletion_queue.into_iter().rev() {
                        o.destroy();
                }

                Ok(vk_img)
        }

        #[rustfmt::skip]
        unsafe fn figure_load_strategy(
                instance: &ash::Instance,
                pdevice: vk::PhysicalDevice,
                src_fmt: vk::Format,
                dst_fmt: vk::Format,
        ) -> VkLoadStrategy {
                let src_fmt_props = instance.get_physical_device_format_properties(pdevice, src_fmt);
                let dst_fmt_props = instance.get_physical_device_format_properties(pdevice, dst_fmt);

                if src_fmt == dst_fmt && dst_fmt_props.optimal_tiling_features.contains(vk::FormatFeatureFlags::TRANSFER_DST) {
                        return VkLoadStrategy::StagingBufferDirect;
                }

                // if src_fmt_props.linear_tiling_features.contains(vk::FormatFeatureFlags::BLIT_SRC)
                //         && dst_fmt_props.optimal_tiling_features.contains(vk::FormatFeatureFlags::BLIT_DST)
                // {
                //         return VkLoadStrategy::LinearImage;
                // }

                if dst_fmt_props.optimal_tiling_features.contains(vk::FormatFeatureFlags::TRANSFER_DST) {
                        return VkLoadStrategy::StagingBufferConvert;
                }

                panic!("no suitable load strategy from {:?} to {:?}", src_fmt, dst_fmt);
        }

        pub unsafe fn new_cubemap(allocator: &Rc<VmaAllocator>, cinfo: &VkImageCubemapCreateInfo) -> VkResult<VkImage> {
                let mip_levels = cinfo.mip_levels.to_value(cinfo.size, cinfo.size);

                let image_cinfo = VkImageCreateInfo {
                        flags: vk::ImageCreateFlags::CUBE_COMPATIBLE,
                        image_type: vk::ImageType::TYPE_2D,
                        format: cinfo.format,
                        extent: vk::Extent3D {
                                width: cinfo.size,
                                height: cinfo.size,
                                depth: 1,
                        },
                        mip_levels,
                        array_layers: 6,
                        samples: vk::SampleCountFlags::TYPE_1,
                        tiling: vk::ImageTiling::OPTIMAL,
                        usage: vk::ImageUsageFlags::TRANSFER_SRC // for creating mipmaps
                                | vk::ImageUsageFlags::TRANSFER_DST
                                | vk::ImageUsageFlags::SAMPLED
                                | cinfo.additional_usage_flags,
                        queue_family_indices: None,
                        initial_layout: vk::ImageLayout::UNDEFINED,
                        mem_usage: vma::MemoryUsage::GpuOnly,
                        alloc_cflags: vma::AllocationCreateFlags::DEDICATED_MEMORY,
                        required_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                        preferred_flags: Default::default(),
                };

                Self::new(Rc::clone(allocator), &image_cinfo)
        }

        pub unsafe fn set_debug_name(&self, device: &VkDevice, debug_utils: &VkDebugUtils, name: &str) -> VkResult<()> {
                let name = CString::new(name).unwrap();
                let name_info = vk::DebugUtilsObjectNameInfoEXT::default()
                        .object_handle(self.handle)
                        .object_name(name.as_c_str());

                debug_utils.device_loader().set_debug_utils_object_name(&name_info)
        }

        pub fn cmd_transition_img_layout(
                device: &ash::Device,
                cmd_buffer: vk::CommandBuffer,
                tinfo: &TransitionImageLayoutInfo,
        ) {
                let image_memory_barrier = vk::ImageMemoryBarrier2 {
                        src_stage_mask: tinfo.src_stage_mask,
                        src_access_mask: tinfo.src_access_mask,
                        dst_stage_mask: tinfo.dst_stage_mask,
                        dst_access_mask: tinfo.dst_access_mask,
                        old_layout: tinfo.old_layout,
                        new_layout: tinfo.new_layout,
                        src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                        dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                        image: tinfo.image,
                        subresource_range: tinfo.subresource_range,
                        ..Default::default()
                };

                let dependency_info =
                        vk::DependencyInfo::default().image_memory_barriers(image_memory_barrier.ref_into_slice());

                unsafe { device.cmd_pipeline_barrier2(cmd_buffer, &dependency_info) };
        }

        pub unsafe fn cmd_copy_image_to_image(
                device: &ash::Device,
                cmd_buffer: vk::CommandBuffer,
                width: u32,
                height: u32,
                src_image: vk::Image,
                dst_image: vk::Image,
                filter: vk::Filter,
        ) {
                let subresource = vk::ImageSubresourceLayers {
                        aspect_mask: vk::ImageAspectFlags::COLOR,
                        mip_level: 0,
                        base_array_layer: 0,
                        layer_count: 1,
                };

                let offsets = [
                        vk::Offset3D { x: 0, y: 0, z: 0 },
                        vk::Offset3D {
                                x: width as i32,
                                y: height as i32,
                                z: 1,
                        },
                ];

                let region = vk::ImageBlit2::default()
                        .src_subresource(subresource)
                        .src_offsets(offsets)
                        .dst_subresource(subresource)
                        .dst_offsets(offsets);

                let regions = [region];

                let blit_image_info = vk::BlitImageInfo2::default()
                        .src_image(src_image)
                        .src_image_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                        .dst_image(dst_image)
                        .dst_image_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                        .regions(&regions)
                        .filter(filter);

                device.cmd_blit_image2(cmd_buffer, &blit_image_info);
        }

        pub fn cmd_copy_buffer_to_image(
                device: &ash::Device,
                cmd_buffer: vk::CommandBuffer,
                width: u32,
                height: u32,
                layers: u32,
                src_buffer: vk::Buffer,
                dst_image: vk::Image,
                dst_image_layout: vk::ImageLayout,
        ) {
                let region = vk::BufferImageCopy {
                        buffer_offset: 0,
                        buffer_row_length: 0,
                        buffer_image_height: 0,
                        image_subresource: vk::ImageSubresourceLayers {
                                aspect_mask: vk::ImageAspectFlags::COLOR,
                                mip_level: 0,
                                base_array_layer: 0,
                                layer_count: layers,
                        },
                        image_offset: vk::Offset3D { x: 0, y: 0, z: 0 },
                        image_extent: vk::Extent3D {
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
                        );
                }
        }

        /// PRE: minfo.image.(mip_level = 0, all_layers) must be in minfo.old_layout layout. The
        /// other mip levels do not matter as they will be overwritten.
        ///
        /// POST: minfo.image.(all_mip_levels, all_layers) will be in minfo.new_layout layout.
        pub fn cmd_gen_mipmaps(minfo: &GenerateMipmapsInfo) {
                assert!(
                        unsafe {
                                minfo.instance
                                        .get_physical_device_format_properties(minfo.pdevice, minfo.image_format)
                                        .optimal_tiling_features
                                        .contains(vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR)
                        },
                        "vk::ImageFormat does not support linear filter!"
                );

                // Transition first mip level to TRANSFER_SRC_OPTIMAL.
                Self::cmd_transition_img_layout(
                        minfo.device,
                        minfo.cmd_buffer,
                        &TransitionImageLayoutInfo {
                                image: minfo.image,
                                old_layout: minfo.old_layout,
                                new_layout: vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                                src_stage_mask: minfo.src_stage_mask,
                                src_access_mask: minfo.src_access_mask,
                                dst_stage_mask: vk::PipelineStageFlags2::TRANSFER,
                                dst_access_mask: vk::AccessFlags2::TRANSFER_READ,
                                subresource_range: vk::ImageSubresourceRange::full_color().level_count(1),
                        },
                );

                // Transition mip levels 1.. to TRANSFER_DST_OPTIMAL.
                Self::cmd_transition_img_layout(
                        minfo.device,
                        minfo.cmd_buffer,
                        &TransitionImageLayoutInfo {
                                image: minfo.image,
                                old_layout: vk::ImageLayout::UNDEFINED,
                                new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                src_stage_mask: vk::PipelineStageFlags2::NONE,
                                src_access_mask: vk::AccessFlags2::NONE,
                                dst_stage_mask: vk::PipelineStageFlags2::TRANSFER,
                                dst_access_mask: vk::AccessFlags2::TRANSFER_WRITE,
                                subresource_range: vk::ImageSubresourceRange::full_color().base_mip_level(1),
                        },
                );

                let mut prev_mip_width = minfo.width;
                let mut prev_mip_height = minfo.height;

                for i in 1..minfo.mip_levels {
                        let this_mip_width = if prev_mip_width > 1 { prev_mip_width / 2 } else { 1 };
                        let this_mip_height = if prev_mip_height > 1 { prev_mip_height / 2 } else { 1 };

                        let mut blit = vk::ImageBlit::default();

                        blit.src_subresource.aspect_mask = vk::ImageAspectFlags::COLOR;
                        blit.src_subresource.mip_level = i - 1;
                        blit.src_subresource.base_array_layer = 0;
                        blit.src_subresource.layer_count = vk::REMAINING_ARRAY_LAYERS;
                        blit.src_offsets[0].x = 0;
                        blit.src_offsets[0].y = 0;
                        blit.src_offsets[0].z = 0;
                        blit.src_offsets[1].x = prev_mip_width as i32;
                        blit.src_offsets[1].y = prev_mip_height as i32;
                        blit.src_offsets[1].z = 1;

                        blit.dst_subresource.aspect_mask = vk::ImageAspectFlags::COLOR;
                        blit.dst_subresource.mip_level = i;
                        blit.dst_subresource.base_array_layer = 0;
                        blit.dst_subresource.layer_count = vk::REMAINING_ARRAY_LAYERS;
                        blit.dst_offsets[0].x = 0;
                        blit.dst_offsets[0].y = 0;
                        blit.dst_offsets[0].z = 0;
                        blit.dst_offsets[1].x = this_mip_width as i32;
                        blit.dst_offsets[1].y = this_mip_height as i32;
                        blit.dst_offsets[1].z = 1;

                        unsafe {
                                minfo.device.cmd_blit_image(
                                        minfo.cmd_buffer,
                                        minfo.image,
                                        vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                                        minfo.image,
                                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                        std::slice::from_ref(&blit),
                                        vk::Filter::LINEAR,
                                );
                        }

                        // Transition the mip level we just created inito TRANSFER_SRC_OPTIMAL for the next mip level.
                        // We could skip this for the last mip level but we don't care.
                        Self::cmd_transition_img_layout(
                                minfo.device,
                                minfo.cmd_buffer,
                                &TransitionImageLayoutInfo {
                                        image: minfo.image,
                                        old_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                                        new_layout: vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                                        src_stage_mask: vk::PipelineStageFlags2::TRANSFER,
                                        src_access_mask: vk::AccessFlags2::TRANSFER_WRITE,
                                        dst_stage_mask: vk::PipelineStageFlags2::TRANSFER,
                                        dst_access_mask: vk::AccessFlags2::TRANSFER_READ,
                                        subresource_range: vk::ImageSubresourceRange::full_color()
                                                .base_mip_level(i)
                                                .level_count(1),
                                },
                        );

                        prev_mip_width = this_mip_width;
                        prev_mip_height = this_mip_height;
                }

                // Now all mip levels are in TRANSFER_SRC_OPTIMAL.
                // Transition all of them into minfo.new_layout.
                Self::cmd_transition_img_layout(
                        minfo.device,
                        minfo.cmd_buffer,
                        &TransitionImageLayoutInfo {
                                image: minfo.image,
                                old_layout: vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                                new_layout: minfo.new_layout,
                                src_stage_mask: vk::PipelineStageFlags2::TRANSFER,
                                src_access_mask: vk::AccessFlags2::NONE,
                                dst_stage_mask: minfo.dst_stage_mask,
                                dst_access_mask: minfo.dst_access_mask,
                                subresource_range: vk::ImageSubresourceRange::full_color(),
                        },
                );
        }
}

impl_destroyable_expr!(VkImage, vk::Image, |s: &VkImage| {
        s.allocator.destroy_image(s.handle, s.alloc);
});

pub struct TransitionImageLayoutInfo {
        pub image: vk::Image,
        pub old_layout: vk::ImageLayout,
        pub new_layout: vk::ImageLayout,
        pub src_stage_mask: vk::PipelineStageFlags2, // NOTE: NONE is equivalent to TOP_OF_PIPE
        pub src_access_mask: vk::AccessFlags2,
        pub dst_stage_mask: vk::PipelineStageFlags2, // NOTE: NONE is equivalent to BOTTOM_OF_PIPE
        pub dst_access_mask: vk::AccessFlags2,
        pub subresource_range: vk::ImageSubresourceRange,
}

pub struct GenerateMipmapsInfo<'a> {
        pub instance: &'a ash::Instance,
        pub pdevice: vk::PhysicalDevice,
        pub device: &'a ash::Device,
        pub cmd_buffer: vk::CommandBuffer,
        pub image: vk::Image,
        pub image_format: vk::Format,
        pub width: u32,
        pub height: u32,
        pub mip_levels: u32,
        pub old_layout: vk::ImageLayout, // The layout of image.(mip_level = 0, all_layers) before the cmd.
        pub new_layout: vk::ImageLayout, // The layout of image.(all_mip_levels, all_layers) after the cmd.
        pub src_stage_mask: vk::PipelineStageFlags2,
        pub src_access_mask: vk::AccessFlags2,
        pub dst_stage_mask: vk::PipelineStageFlags2,
        pub dst_access_mask: vk::AccessFlags2,
}

#[derive(Debug, PartialEq, Eq)]
enum VkLoadStrategy {
        StagingBufferDirect,
        StagingBufferConvert,
        LinearImage,
}
