use std::{cell::Cell, ffi::CString, ops::Deref, rc::Rc};

use ash::{
        prelude::VkResult,
        vk::{self, Handle},
};
#[allow(unused_imports)]
use log::{debug, error, info, trace, warn};
use vk_mem::Alloc;

use crate::{
        asset_manager::{Cubemap, Image},
        AnyResult,
};

use super::{
        vk_buffer::VkBuffer,
        vk_command_buffer::VkReusableCommandBuffer,
        vk_util::vk_format_from_image_format_and_color_space,
        vk_wrapper::{
                impl_destroyable_deref, impl_destroyable_drop, impl_destroyable_expr, VkDebugUtils, VkDevice, VkObject,
                VmaAllocator,
        },
};

#[allow(dead_code)]
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

pub struct VkImageCreateFromDataInfo<'a> {
        pub data: &'a [u8],
        pub width: u32,
        pub height: u32,
        pub format: vk::Format,
        pub mip_levels: MipLevels,
        pub samples: vk::SampleCountFlags,
        pub setup_cmd_buffer: &'a VkReusableCommandBuffer,
        pub transfer_queue: vk::Queue,
}

pub struct VkImageCubemapCreateInfo<'a> {
        pub cubemap: &'a Cubemap,
        pub mip_levels: MipLevels,
        pub setup_cmd_buffer: &'a VkReusableCommandBuffer,
        pub transfer_queue: vk::Queue,
}

#[allow(dead_code)]
pub struct VkImage {
        allocator: Rc<VmaAllocator>,

        handle: vk::Image,
        alloc: vma::Allocation,

        destroyed: Cell<bool>,

        pub format: vk::Format,
        pub mip_levels: u32,
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
                        mip_levels: create_info.mip_levels,
                })
        }

        pub unsafe fn from_data(
                instance: &ash::Instance,
                pdevice: &vk::PhysicalDevice,
                device: &ash::Device,
                allocator: Rc<VmaAllocator>,
                cinfo: &VkImageCreateFromDataInfo,
        ) -> AnyResult<Self> {
                let mip_levels = cinfo.mip_levels.to_value(cinfo.width, cinfo.height);

                let final_format = match cinfo.format {
                        vk::Format::R8_UNORM
                        | vk::Format::R8G8B8A8_SRGB
                        | vk::Format::R8G8B8A8_UNORM
                        | vk::Format::R16G16B16A16_SFLOAT => cinfo.format,
                        vk::Format::R8G8B8_SRGB => vk::Format::R8G8B8A8_SRGB,
                        vk::Format::R8G8B8_UNORM => vk::Format::R8G8B8A8_UNORM,
                        _ => panic!("Unsupported vk::Format! {:?}", cinfo.format),
                };

                let bytes_per_pixel = Self::get_bytes_per_pixel_for_vk_format(final_format);

                let buffer_size = (cinfo.width * cinfo.height * bytes_per_pixel) as vk::DeviceSize;
                let staging_buffer = VkBuffer::new_transfer_src(device, Rc::clone(&allocator), buffer_size)?;

                match (cinfo.format, final_format) {
                        (i, f) if i == f => {
                                staging_buffer.write_bytes(cinfo.data)?;
                        },
                        (vk::Format::R8G8B8_SRGB, vk::Format::R8G8B8A8_SRGB)
                        | (vk::Format::R8G8B8_UNORM, vk::Format::R8G8B8A8_UNORM) => {
                                assert_eq!(cinfo.data.len() % 3, 0);
                                warn!("slow format: {:?}", cinfo.format);

                                for (i, rgb) in cinfo.data.chunks(3).enumerate() {
                                        staging_buffer
                                                .write_bytes_offsetted(&[rgb[0], rgb[1], rgb[2], u8::MAX], i * 4)?;
                                }
                        },
                        _ => panic!(
                                "unsupported (initial, final) vk::Format pair ({:?}, {:?})",
                                cinfo.format, final_format
                        ),
                }
                staging_buffer.unmap_memory();

                let format = final_format;
                let vk_img_cinfo = VkImageCreateInfo {
                        flags: Default::default(),
                        image_type: vk::ImageType::TYPE_2D,
                        format,
                        extent: vk::Extent3D {
                                width: cinfo.width,
                                height: cinfo.height,
                                depth: 1,
                        },
                        mip_levels,
                        array_layers: 1,
                        samples: cinfo.samples,
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

                let vk_img = VkImage::new(allocator, &vk_img_cinfo)?;

                let cmd_buffer = **cinfo.setup_cmd_buffer;

                cinfo.setup_cmd_buffer.begin(device)?;

                Self::cmd_transition_img_layout(&TransitionImageLayoutInfo {
                        device,
                        cmd_buffer,

                        old_layout: vk::ImageLayout::UNDEFINED,
                        new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,

                        image: *vk_img,
                        base_mip_level: 0,
                        mip_levels,
                        base_array_layer: 0,
                        layer_count: 1,
                        aspect_mask: vk::ImageAspectFlags::COLOR,

                        src_access_mask: vk::AccessFlags::empty(),
                        dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,

                        src_stage_mask: vk::PipelineStageFlags::TOP_OF_PIPE,
                        dst_stage_mask: vk::PipelineStageFlags::TRANSFER,
                });

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

                Self::cmd_gen_mipmaps(&GenerateMipmapsInfo {
                        instance,
                        pdevice,
                        device,
                        cmd_buffer,
                        image: *vk_img,
                        image_format: format,
                        width: cinfo.width,
                        height: cinfo.height,
                        mip_levels,
                        base_array_layer: 0,
                        layer_count: 1,
                });

                cinfo.setup_cmd_buffer
                        .end_and_submit(device, cinfo.transfer_queue, &[], &[], &[])?;

                cinfo.setup_cmd_buffer.wait(u64::MAX)?;
                staging_buffer.destroy();

                Ok(vk_img)
        }

        fn get_bytes_per_pixel_for_vk_format(format: vk::Format) -> u32 {
                if vk::Format::R8_UNORM <= format && format <= vk::Format::R8_SRGB {
                        1
                } else if vk::Format::R8G8_UNORM <= format && format <= vk::Format::R8G8_SRGB {
                        2
                } else if vk::Format::R8G8B8_UNORM <= format && format <= vk::Format::B8G8R8_SRGB {
                        3
                } else if vk::Format::R8G8B8A8_UNORM <= format && format <= vk::Format::A2B10G10R10_SINT_PACK32 {
                        4
                } else if vk::Format::R16G16B16A16_UNORM <= format && format <= vk::Format::R16G16B16A16_SFLOAT {
                        8
                } else {
                        panic!("unsupported image format {:?}", format);
                }
        }

        pub unsafe fn new_cubemap(
                instance: &ash::Instance,
                pdevice: &vk::PhysicalDevice,
                device: &ash::Device,
                allocator: &Rc<VmaAllocator>,
                cinfo: &VkImageCubemapCreateInfo,
        ) -> AnyResult<VkImage> {
                let size = match cinfo.cubemap {
                        Cubemap::Faces(faces) => {
                                let size = faces[0].width;
                                for f in faces {
                                        assert_eq!(f.width, size);
                                        assert_eq!(f.height, size);
                                }
                                size
                        },
                        Cubemap::Equirectangular(image) => {
                                // We divide width by 4, as there are four horizontal faces.
                                let size = image.width / 4;
                                size
                        },
                };

                let format = match cinfo.cubemap {
                        Cubemap::Faces(faces) => {
                                let format = vk_format_from_image_format_and_color_space(
                                        faces[0].format,
                                        faces[0].color_space,
                                );

                                for f in &faces[1..] {
                                        assert_eq!(
                                                format,
                                                vk_format_from_image_format_and_color_space(f.format, f.color_space)
                                        );
                                }

                                format
                        },
                        Cubemap::Equirectangular(image) => {
                                vk_format_from_image_format_and_color_space(image.format, image.color_space)
                        },
                };

                let mip_levels = cinfo.mip_levels.to_value(size, size);

                let image_cinfo = VkImageCreateInfo {
                        flags: vk::ImageCreateFlags::CUBE_COMPATIBLE,
                        image_type: vk::ImageType::TYPE_2D,
                        format,
                        extent: vk::Extent3D {
                                width: size,
                                height: size,
                                depth: 1,
                        },
                        mip_levels,
                        array_layers: 6,
                        samples: vk::SampleCountFlags::TYPE_1,
                        tiling: vk::ImageTiling::OPTIMAL,
                        usage: vk::ImageUsageFlags::TRANSFER_SRC // for creating mipmaps
                                | vk::ImageUsageFlags::TRANSFER_DST
                                | vk::ImageUsageFlags::SAMPLED,
                        queue_family_indices: None,
                        initial_layout: vk::ImageLayout::UNDEFINED,
                        mem_usage: vma::MemoryUsage::GpuOnly,
                        alloc_cflags: vma::AllocationCreateFlags::DEDICATED_MEMORY,
                        required_flags: vk::MemoryPropertyFlags::DEVICE_LOCAL,
                        preferred_flags: Default::default(),
                };

                let dst_image = Self::new(Rc::clone(allocator), &image_cinfo)?;

                cinfo.setup_cmd_buffer.begin(device)?;

                let cmd_buffer = **cinfo.setup_cmd_buffer;
                let mut deletion_queue = vec![];

                Self::cmd_transition_img_layout(&TransitionImageLayoutInfo {
                        device,
                        cmd_buffer,

                        old_layout: vk::ImageLayout::UNDEFINED,
                        new_layout: vk::ImageLayout::TRANSFER_DST_OPTIMAL,

                        image: *dst_image,
                        base_mip_level: 0,
                        mip_levels,
                        base_array_layer: 0,
                        layer_count: 6,
                        aspect_mask: vk::ImageAspectFlags::COLOR,

                        src_access_mask: vk::AccessFlags::empty(),
                        dst_access_mask: vk::AccessFlags::TRANSFER_WRITE,

                        src_stage_mask: vk::PipelineStageFlags::TOP_OF_PIPE,
                        dst_stage_mask: vk::PipelineStageFlags::TRANSFER,
                });

                match cinfo.cubemap {
                        Cubemap::Faces(faces) => {
                                let faces_data = [
                                        faces[0].pixels.as_slice(),
                                        faces[1].pixels.as_slice(),
                                        faces[2].pixels.as_slice(),
                                        faces[3].pixels.as_slice(),
                                        faces[4].pixels.as_slice(),
                                        faces[5].pixels.as_slice(),
                                ];

                                Self::cmd_transfer_faces_into_cubemap(
                                        device,
                                        allocator,
                                        cmd_buffer,
                                        &mut deletion_queue,
                                        faces_data,
                                        *dst_image,
                                        format,
                                        size,
                                )?;
                        },
                        Cubemap::Equirectangular(image) => {
                                // Self::cmd_transfer_equirectangular_into_cubemap()?;
                        },
                };

                Self::cmd_gen_mipmaps(&GenerateMipmapsInfo {
                        instance,
                        pdevice,
                        device,
                        cmd_buffer,
                        image: *dst_image,
                        image_format: format,
                        width: size,
                        height: size,
                        mip_levels,
                        base_array_layer: 0,
                        layer_count: 6,
                });

                cinfo.setup_cmd_buffer
                        .end_and_submit(device, cinfo.transfer_queue, &[], &[], &[])?;

                cinfo.setup_cmd_buffer.wait(u64::MAX)?;

                for o in deletion_queue {
                        match o {
                                VkObject::Buffer(b) => b.destroy(),
                                VkObject::Image(i) => i.destroy(),
                        };
                }

                Ok(dst_image)
        }

        // `dst_image` must have `format` format, and all byte arrays in `faces_data`
        // must be compatible with `format`.
        unsafe fn cmd_transfer_faces_into_cubemap(
                device: &ash::Device,
                allocator: &Rc<VmaAllocator>,
                cmd_buffer: vk::CommandBuffer,
                cmd_buffer_deletion_queue: &mut Vec<VkObject>,
                faces_data: [&[u8]; 6],
                dst_image: vk::Image,
                format: vk::Format,
                size: u32,
        ) -> VkResult<()> {
                let bytes_per_pixel = Self::get_bytes_per_pixel_for_vk_format(format);
                let buffer_size = 6 * (size * size * bytes_per_pixel) as vk::DeviceSize;
                let staging_buffer = VkBuffer::new_transfer_src(device, Rc::clone(&allocator), buffer_size)?;

                let mut offset = 0;
                for face_data in faces_data {
                        staging_buffer.write_bytes_offsetted(face_data, offset)?;
                        offset += face_data.len();
                }
                staging_buffer.unmap_memory();

                Self::cmd_copy_buffer_to_image(
                        device,
                        cmd_buffer,
                        size,
                        size,
                        6,
                        *staging_buffer,
                        dst_image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                );

                cmd_buffer_deletion_queue.push(VkObject::Buffer(staging_buffer));

                Ok(())
        }

        unsafe fn cmd_transfer_equirectangular_into_cubemap(
                device: &VkDevice,
                allocator: &Rc<VmaAllocator>,
                cubemap_pipeline: vk::Pipeline,
                cmd_buffer: vk::CommandBuffer,
                cmd_buffer_deletion_queue: &mut Vec<VkObject>,
                equirectangular_image: &Image,
                dst_image: vk::Image,
                format: vk::Format,
                size: u32,
        ) -> VkResult<()> {
                Ok(())
        }

        pub unsafe fn set_debug_name(&self, device: &VkDevice, debug_utils: &VkDebugUtils, name: &str) -> VkResult<()> {
                let name = CString::new(name).unwrap();
                let name_info = vk::DebugUtilsObjectNameInfoEXT::builder()
                        .object_handle(self.handle.as_raw())
                        .object_type(vk::ObjectType::IMAGE)
                        .object_name(name.as_c_str());

                debug_utils
                        .loader()
                        .set_debug_utils_object_name(device.handle(), &name_info)
        }

        pub fn cmd_transition_img_layout(tinfo: &TransitionImageLayoutInfo) {
                let barrier = vk::ImageMemoryBarrier {
                        src_access_mask: tinfo.src_access_mask,
                        dst_access_mask: tinfo.dst_access_mask,
                        old_layout: tinfo.old_layout,
                        new_layout: tinfo.new_layout,
                        src_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                        dst_queue_family_index: vk::QUEUE_FAMILY_IGNORED,
                        image: tinfo.image,
                        subresource_range: vk::ImageSubresourceRange {
                                aspect_mask: tinfo.aspect_mask,
                                base_mip_level: tinfo.base_mip_level,
                                level_count: tinfo.mip_levels,
                                base_array_layer: tinfo.base_array_layer,
                                layer_count: tinfo.layer_count,
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
                        );
                }
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

                let region = vk::ImageBlit2::builder()
                        .src_subresource(subresource)
                        .src_offsets(offsets)
                        .dst_subresource(subresource)
                        .dst_offsets(offsets)
                        .build();

                let regions = [region];

                let blit_image_info = vk::BlitImageInfo2::builder()
                        .src_image(src_image)
                        .src_image_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                        .dst_image(dst_image)
                        .dst_image_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                        .regions(&regions)
                        .filter(filter);

                device.cmd_blit_image2(cmd_buffer, &blit_image_info);
        }

        fn cmd_copy_buffer_to_image(
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

        fn cmd_gen_mipmaps(minfo: &GenerateMipmapsInfo) {
                assert!(
                        unsafe {
                                minfo.instance
                                        .get_physical_device_format_properties(*minfo.pdevice, minfo.image_format)
                                        .optimal_tiling_features
                                        .contains(vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR)
                        },
                        "vk::ImageFormat does not support linear filter!"
                );

                let mut tinfo = TransitionImageLayoutInfo {
                        device: minfo.device,
                        cmd_buffer: minfo.cmd_buffer,

                        image: minfo.image,
                        aspect_mask: vk::ImageAspectFlags::COLOR,

                        base_mip_level: Default::default(),
                        mip_levels: 1,
                        base_array_layer: minfo.base_array_layer,
                        layer_count: minfo.layer_count,

                        src_access_mask: Default::default(),
                        dst_access_mask: Default::default(),
                        old_layout: Default::default(),
                        new_layout: Default::default(),

                        src_stage_mask: Default::default(),
                        dst_stage_mask: Default::default(),
                };

                let mut prev_mip_width = minfo.width;
                let mut prev_mip_height = minfo.height;

                for i in 1..minfo.mip_levels {
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
                        blit.src_subresource.base_array_layer = minfo.base_array_layer;
                        blit.src_subresource.layer_count = minfo.layer_count;
                        blit.src_offsets[0].x = 0;
                        blit.src_offsets[0].y = 0;
                        blit.src_offsets[0].z = 0;
                        blit.src_offsets[1].x = prev_mip_width as i32;
                        blit.src_offsets[1].y = prev_mip_height as i32;
                        blit.src_offsets[1].z = 1;

                        blit.dst_subresource.aspect_mask = vk::ImageAspectFlags::COLOR;
                        blit.dst_subresource.mip_level = i;
                        blit.dst_subresource.base_array_layer = minfo.base_array_layer;
                        blit.dst_subresource.layer_count = minfo.layer_count;
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

                tinfo.base_mip_level = minfo.mip_levels - 1;
                tinfo.old_layout = vk::ImageLayout::TRANSFER_DST_OPTIMAL;
                tinfo.new_layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
                tinfo.src_access_mask = vk::AccessFlags::TRANSFER_WRITE;
                tinfo.dst_access_mask = vk::AccessFlags::SHADER_READ;
                tinfo.src_stage_mask = vk::PipelineStageFlags::TRANSFER;
                tinfo.dst_stage_mask = vk::PipelineStageFlags::FRAGMENT_SHADER;

                Self::cmd_transition_img_layout(&tinfo);
        }
}

impl_destroyable_expr!(VkImage, vk::Image, |s: &VkImage| {
        s.allocator.destroy_image(s.handle, s.alloc);
});

pub struct TransitionImageLayoutInfo<'a> {
        pub device: &'a ash::Device,
        pub cmd_buffer: vk::CommandBuffer,
        pub image: vk::Image,
        pub base_mip_level: u32,
        pub mip_levels: u32,
        pub base_array_layer: u32,
        pub layer_count: u32,
        pub aspect_mask: vk::ImageAspectFlags,
        pub src_stage_mask: vk::PipelineStageFlags,
        pub dst_stage_mask: vk::PipelineStageFlags,
        pub src_access_mask: vk::AccessFlags,
        pub dst_access_mask: vk::AccessFlags,
        pub old_layout: vk::ImageLayout,
        pub new_layout: vk::ImageLayout,
}

struct GenerateMipmapsInfo<'a> {
        instance: &'a ash::Instance,
        pdevice: &'a vk::PhysicalDevice,
        device: &'a ash::Device,
        cmd_buffer: vk::CommandBuffer,
        image: vk::Image,
        image_format: vk::Format,
        width: u32,
        height: u32,
        mip_levels: u32,
        base_array_layer: u32,
        layer_count: u32,
}
