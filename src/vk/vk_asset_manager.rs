use std::{error::Error, rc::Rc};

use ash::vk;
use log::trace;

use crate::{
        asset_manager::{
                Buffer, BufferID, BufferView, BufferViewID, ComponentType, DataType, Image, ImageFormat, ImageID,
                MagFilter, MinFilter, Sampler, SamplerID, WrappingMode,
        },
        constants::{ENABLE_ANISOTROPY, LOD_CLAMP_NONE},
        vec_map::VecMap,
        vk::{
                vk_buffer::{BufferData, VkBuffer, VkImmutableBufferCreateInfo},
                vk_command_buffer::VkReusableCommandBuffer,
                vk_image::{MipLevels, VkImage, VkImageCreateFromDataInfo},
                vk_wrapper::{VkCommandPool, VkDevice, VkImageView, VkPhysicalDevice, VkSampler},
        },
};

struct VkModelBufferView {
        buffer: VkBuffer,
        format: vk::Format,
        element_count: usize,
}

struct VkModelImage {
        image: VkImage,
        image_view: VkImageView,
}

pub struct VkAssetManager {
        buffer_views: VecMap<BufferViewID, VkModelBufferView>,
        images: VecMap<ImageID, VkModelImage>,
        samplers: VecMap<SamplerID, VkSampler>,
}

impl VkAssetManager {
        pub fn new(
                instance: &ash::Instance,
                pdevice: &VkPhysicalDevice,
                device: &Rc<VkDevice>,
                allocator: &Rc<vma::Allocator>,
                transfer_queue: vk::Queue,
                cmd_pool: &Rc<VkCommandPool>,
                buffers: &VecMap<BufferID, Buffer>,
                buffer_views: &VecMap<BufferViewID, BufferView>,
                images: &VecMap<ImageID, Image>,
                samplers: &VecMap<SamplerID, Sampler>,
        ) -> Result<Self, Box<dyn Error>> {
                let cmd_buffer = VkReusableCommandBuffer::new(device, cmd_pool)?;

                trace!("Creating VkBuffers...");
                let vk_buffer_views = Self::create_vk_buffers_from_buffers(
                        device,
                        allocator,
                        transfer_queue,
                        &cmd_buffer,
                        buffers,
                        buffer_views,
                )?;

                trace!("Creating VkImages...");
                let vk_images = Self::create_vk_images_from_images(instance, pdevice, device, allocator, transfer_queue, &cmd_buffer, images)?;

                trace!("Creating VkSamplers...");
                let vk_samplers = Self::create_vk_samplers_from_samplers(pdevice, device, samplers)?;

                Ok(Self {
                        buffer_views: vk_buffer_views,
                        images: vk_images,
                        samplers: vk_samplers,
                })
        }

        fn create_vk_buffers_from_buffers(
                device: &Rc<VkDevice>,
                allocator: &Rc<vma::Allocator>,
                transfer_queue: vk::Queue,
                cmd_buffer: &VkReusableCommandBuffer,
                buffers: &VecMap<BufferID, Buffer>,
                buffer_views: &VecMap<BufferViewID, BufferView>,
        ) -> Result<VecMap<BufferViewID, VkModelBufferView>, Box<dyn Error>> {
                let mut vk_buffer_views: VecMap<BufferViewID, VkModelBufferView> = VecMap::new();

                for (bview_id, bview) in buffer_views {
                        let buffer = &buffers[bview.buffer_id];

                        assert!(buffer.byte_length >= (bview.byte_offset + bview.byte_length));

                        let vk_buffer_cinfo = VkImmutableBufferCreateInfo {
                                device,
                                allocator,
                                cmd_buffer: &cmd_buffer,
                                transfer_queue,
                                buffer_usage: vk::BufferUsageFlags::VERTEX_BUFFER | vk::BufferUsageFlags::INDEX_BUFFER,
                                data: BufferData::OffsetLength {
                                        data: buffer.bytes.as_slice(),
                                        offset: bview.byte_offset,
                                        length: bview.byte_length,
                                },
                        };

                        let vk_buffer = VkBuffer::new_immutable(&vk_buffer_cinfo)?;

                        let vk_bview_id = vk_buffer_views.insert(VkModelBufferView {
                                buffer: vk_buffer,
                                format: Self::vk_format_from_component_and_data_type(
                                        bview.component_type,
                                        bview.data_type,
                                ),
                                element_count: bview.element_count,
                        });

                        assert_eq!(bview_id, vk_bview_id);
                }

                Ok(vk_buffer_views)
        }

        fn create_vk_images_from_images(
                instance: &ash::Instance,
                pdevice: &VkPhysicalDevice,
                device: &Rc<VkDevice>,
                allocator: &Rc<vma::Allocator>,
                transfer_queue: vk::Queue,
                cmd_buffer: &VkReusableCommandBuffer,
                images: &VecMap<ImageID, Image>,
        ) -> Result<VecMap<ImageID, VkModelImage>, Box<dyn Error>> {
                let mut vk_images: VecMap<ImageID, VkModelImage> = VecMap::new();

                for (image_id, image) in images {
                        let vk_image_cinfo = VkImageCreateFromDataInfo {
                                data: &image.pixels,
                                width: image.width,
                                height: image.height,
                                format: Self::vk_format_from_image_format(image.format),
                                mip_levels: MipLevels::Log2,
                                samples: vk::SampleCountFlags::TYPE_1,
                                setup_cmd_buffer: &cmd_buffer,
                                transfer_queue,
                        };

                        let vk_image =
                                unsafe { VkImage::from_data(instance, pdevice, device, allocator, &vk_image_cinfo)? };

                        let vk_image_view_cinfo = vk::ImageViewCreateInfo {
                                image: *vk_image,
                                view_type: vk::ImageViewType::TYPE_2D,
                                format: vk_image_cinfo.format,
                                components: Default::default(),
                                subresource_range: vk::ImageSubresourceRange {
                                        aspect_mask: vk::ImageAspectFlags::COLOR,
                                        base_mip_level: 0,
                                        level_count: vk_image.mip_levels,
                                        base_array_layer: 0,
                                        layer_count: 1,
                                },
                                ..Default::default()
                        };

                        let vk_image_view = unsafe { VkImageView::new(device, &vk_image_view_cinfo)? };

                        let vk_image_id = vk_images.insert(VkModelImage {
                                image: vk_image,
                                image_view: vk_image_view,
                        });

                        assert_eq!(image_id, vk_image_id);
                }

                Ok(vk_images)
        }

        fn create_vk_samplers_from_samplers(
                pdevice: &VkPhysicalDevice,
                device: &Rc<VkDevice>,
                samplers: &VecMap<SamplerID, Sampler>,
        ) -> Result<VecMap<SamplerID, VkSampler>, Box<dyn Error>> {
                let mut vk_samplers = VecMap::<SamplerID, VkSampler>::new();

                for (sampler_id, sampler) in samplers {
                        let vk_sampler_cinfo = vk::SamplerCreateInfo {
                                mag_filter: Self::vk_filter_from_mag_filter(sampler.mag_filter),
                                min_filter: Self::vk_filter_from_min_filter(sampler.min_filter),
                                mipmap_mode: Self::vk_sampler_mipmap_mode_from_min_filter(sampler.min_filter),
                                address_mode_u: Self::vk_sampler_address_mode_from_wrapping_mode(sampler.wrap_s),
                                address_mode_v: Self::vk_sampler_address_mode_from_wrapping_mode(sampler.wrap_t),
                                address_mode_w: vk::SamplerAddressMode::REPEAT,
                                mip_lod_bias: 0.0,
                                anisotropy_enable: ENABLE_ANISOTROPY as vk::Bool32,
                                max_anisotropy: pdevice.max_sampler_anisotropy,
                                compare_enable: vk::FALSE,
                                compare_op: vk::CompareOp::ALWAYS,
                                min_lod: 0.0,
                                max_lod: LOD_CLAMP_NONE,
                                border_color: vk::BorderColor::INT_OPAQUE_BLACK,
                                unnormalized_coordinates: vk::FALSE,
                                ..Default::default()
                        };

                        let vk_sampler = unsafe { VkSampler::new(device, &vk_sampler_cinfo)? };
                        let vk_sampler_id = vk_samplers.insert(vk_sampler);

                        assert_eq!(sampler_id, vk_sampler_id);
                }

                Ok(vk_samplers)
        }

        fn vk_format_from_component_and_data_type(comp_type: ComponentType, data_type: DataType) -> vk::Format {
                match (comp_type, data_type) {
                        (ComponentType::I8, DataType::Scalar) => vk::Format::R8_SINT,
                        (ComponentType::I16, DataType::Scalar) => vk::Format::R16_SINT,
                        (ComponentType::U8, DataType::Scalar) => vk::Format::R8_UINT,
                        (ComponentType::U16, DataType::Scalar) => vk::Format::R16_UINT,
                        (ComponentType::U32, DataType::Scalar) => vk::Format::R32_UINT,
                        (ComponentType::F32, DataType::Scalar) => vk::Format::R32_SFLOAT,

                        (ComponentType::I8, DataType::Vec2) => vk::Format::R8G8_SINT,
                        (ComponentType::I16, DataType::Vec2) => vk::Format::R16G16_SINT,
                        (ComponentType::U8, DataType::Vec2) => vk::Format::R8G8_UINT,
                        (ComponentType::U16, DataType::Vec2) => vk::Format::R16G16_UINT,
                        (ComponentType::U32, DataType::Vec2) => vk::Format::R32G32_UINT,
                        (ComponentType::F32, DataType::Vec2) => vk::Format::R32G32_SFLOAT,

                        (ComponentType::I8, DataType::Vec3) => vk::Format::R8G8B8_SINT,
                        (ComponentType::I16, DataType::Vec3) => vk::Format::R16G16B16_SINT,
                        (ComponentType::U8, DataType::Vec3) => vk::Format::R8G8B8_UINT,
                        (ComponentType::U16, DataType::Vec3) => vk::Format::R16G16B16_UINT,
                        (ComponentType::U32, DataType::Vec3) => vk::Format::R32G32B32_UINT,
                        (ComponentType::F32, DataType::Vec3) => vk::Format::R32G32B32_SFLOAT,

                        (ComponentType::I8, DataType::Vec4) => vk::Format::R8G8B8A8_SINT,
                        (ComponentType::I16, DataType::Vec4) => vk::Format::R16G16B16A16_SINT,
                        (ComponentType::U8, DataType::Vec4) => vk::Format::R8G8B8A8_UINT,
                        (ComponentType::U16, DataType::Vec4) => vk::Format::R16G16B16A16_UINT,
                        (ComponentType::U32, DataType::Vec4) => vk::Format::R32G32B32A32_UINT,
                        (ComponentType::F32, DataType::Vec4) => vk::Format::R32G32B32A32_SFLOAT,

                        _ => panic!("vk::Format from ({:?}, {:?}) not supported!", comp_type, data_type),
                }
        }

        fn vk_format_from_image_format(img_format: ImageFormat) -> vk::Format {
                match img_format {
                        ImageFormat::R8 => vk::Format::R8_SRGB,
                        ImageFormat::R8G8 => vk::Format::R8G8_SRGB,
                        ImageFormat::R8G8B8 => vk::Format::R8G8B8_SRGB,
                        ImageFormat::R8G8B8A8 => vk::Format::R8G8B8A8_SRGB,
                        ImageFormat::B8G8R8 => vk::Format::B8G8R8_SRGB,
                        ImageFormat::B8G8R8A8 => vk::Format::B8G8R8A8_SRGB,
                        ImageFormat::R16 => vk::Format::R16_UINT,
                        ImageFormat::R16G16 => vk::Format::R16G16_UINT,
                        ImageFormat::R16G16B16 => vk::Format::R16G16B16_UINT,
                        ImageFormat::R16G16B16A16 => vk::Format::R16G16B16A16_UINT,
                }
        }

        fn vk_filter_from_mag_filter(mag_filter: MagFilter) -> vk::Filter {
                match mag_filter {
                        MagFilter::Nearest => vk::Filter::NEAREST,
                        MagFilter::Linear => vk::Filter::LINEAR,
                }
        }

        fn vk_filter_from_min_filter(min_filter: MinFilter) -> vk::Filter {
                match min_filter {
                        MinFilter::Nearest => vk::Filter::NEAREST,
                        MinFilter::NearestMipmapNearest => vk::Filter::NEAREST,
                        MinFilter::NearestMipmapLinear => vk::Filter::NEAREST,
                        MinFilter::Linear => vk::Filter::LINEAR,
                        MinFilter::LinearMipmapNearest => vk::Filter::LINEAR,
                        MinFilter::LinearMipmapLinear => vk::Filter::LINEAR,
                }
        }

        fn vk_sampler_mipmap_mode_from_min_filter(min_filter: MinFilter) -> vk::SamplerMipmapMode {
                match min_filter {
                        MinFilter::NearestMipmapLinear => vk::SamplerMipmapMode::LINEAR,
                        MinFilter::LinearMipmapLinear => vk::SamplerMipmapMode::LINEAR,
                        MinFilter::NearestMipmapNearest => vk::SamplerMipmapMode::NEAREST,
                        MinFilter::LinearMipmapNearest => vk::SamplerMipmapMode::NEAREST,
                        _ => vk::SamplerMipmapMode::LINEAR,
                }
        }

        fn vk_sampler_address_mode_from_wrapping_mode(wrapping_mode: WrappingMode) -> vk::SamplerAddressMode {
                match wrapping_mode {
                        WrappingMode::ClampToEdge => vk::SamplerAddressMode::CLAMP_TO_EDGE,
                        WrappingMode::MirroredRepeat => vk::SamplerAddressMode::MIRRORED_REPEAT,
                        WrappingMode::Repeat => vk::SamplerAddressMode::REPEAT,
                }
        }
}
