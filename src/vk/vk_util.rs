use ash::{prelude::VkResult, vk};

use crate::asset_manager::{ColorSpace, ImageFormat};

use super::vk_wrapper::VkInstance;

/// Will choose the first suitable candidate, or return Err if none is suitable.
pub fn find_best_format_for_optimal_tiling(
        instance: &VkInstance,
        physical_device: vk::PhysicalDevice,
        candidates: &[vk::Format],
        features: vk::FormatFeatureFlags,
) -> VkResult<vk::Format> {
        candidates
                .iter()
                .cloned()
                .find(|&format| {
                        let format_props =
                                unsafe { instance.get_physical_device_format_properties(physical_device, format) };

                        (format_props.optimal_tiling_features & features) == features
                })
                .ok_or(vk::Result::ERROR_FORMAT_NOT_SUPPORTED)
}

pub fn vk_format_from_image_format_and_color_space(img_format: ImageFormat, color_space: ColorSpace) -> vk::Format {
        match (img_format, color_space) {
                (ImageFormat::R8, ColorSpace::Srgb) => vk::Format::R8_SRGB,
                // (ImageFormat::R8G8, ColorSpace::Srgb) => vk::Format::R8G8_SRGB,
                (ImageFormat::R8G8B8, ColorSpace::Srgb) => vk::Format::R8G8B8_SRGB,
                (ImageFormat::R8G8B8A8, ColorSpace::Srgb) => vk::Format::R8G8B8A8_SRGB,
                // (ImageFormat::B8G8R8, ColorSpace::Srgb) => vk::Format::B8G8R8_SRGB,
                // (ImageFormat::B8G8R8A8, ColorSpace::Srgb) => vk::Format::B8G8R8A8_SRGB,
                (ImageFormat::R8, ColorSpace::Linear) => vk::Format::R8_UNORM,
                // (ImageFormat::R8G8, ColorSpace::Linear) => vk::Format::R8G8_UNORM,
                (ImageFormat::R8G8B8, ColorSpace::Linear) => vk::Format::R8G8B8_UNORM,
                (ImageFormat::R8G8B8A8, ColorSpace::Linear) => vk::Format::R8G8B8A8_UNORM,
                // (ImageFormat::B8G8R8, ColorSpace::Linear) => vk::Format::B8G8R8_UNORM,
                // (ImageFormat::B8G8R8A8, ColorSpace::Linear) => vk::Format::B8G8R8A8_UNORM,
                // (ImageFormat::R16, ColorSpace::Linear) => vk::Format::R16_UINT,
                // (ImageFormat::R16G16, ColorSpace::Linear) => vk::Format::R16G16_UINT,
                // (ImageFormat::R16G16B16, ColorSpace::Linear) => vk::Format::R16G16B16_UINT,
                // (ImageFormat::R16G16B16A16, ColorSpace::Linear) => vk::Format::R16G16B16A16_UINT,
                (ImageFormat::R16G16B16A16, ColorSpace::Linear) => vk::Format::R16G16B16A16_SFLOAT,
                (ImageFormat::R32G32B32, ColorSpace::Linear) => vk::Format::R32G32B32_SFLOAT,
                (ImageFormat::R32G32B32A32, ColorSpace::Linear) => vk::Format::R32G32B32A32_SFLOAT,
                (ImageFormat::BC1_UNORM, ColorSpace::Linear) => vk::Format::BC1_RGBA_UNORM_BLOCK,
                (ImageFormat::BC1_UNORM, ColorSpace::Srgb) => vk::Format::BC1_RGBA_SRGB_BLOCK,
                (ImageFormat::BC2_UNORM, ColorSpace::Linear) => vk::Format::BC2_UNORM_BLOCK,
                (ImageFormat::BC2_UNORM, ColorSpace::Srgb) => vk::Format::BC2_SRGB_BLOCK,
                (ImageFormat::BC3_UNORM, ColorSpace::Linear) => vk::Format::BC3_UNORM_BLOCK,
                (ImageFormat::BC3_UNORM, ColorSpace::Srgb) => vk::Format::BC3_SRGB_BLOCK,
                (ImageFormat::BC4_UNORM, ColorSpace::Linear) => vk::Format::BC4_UNORM_BLOCK,
                (ImageFormat::BC4_SNORM, ColorSpace::Linear) => vk::Format::BC4_SNORM_BLOCK,
                (ImageFormat::BC5_UNORM, ColorSpace::Linear) => vk::Format::BC5_UNORM_BLOCK,
                (ImageFormat::BC5_SNORM, ColorSpace::Linear) => vk::Format::BC5_SNORM_BLOCK,
                (ImageFormat::BC6H_UFLOAT, ColorSpace::Linear) => vk::Format::BC6H_UFLOAT_BLOCK,
                (ImageFormat::BC6H_SFLOAT, ColorSpace::Linear) => vk::Format::BC6H_SFLOAT_BLOCK,
                (ImageFormat::BC7_UNORM, ColorSpace::Linear) => vk::Format::BC7_UNORM_BLOCK,
                (ImageFormat::BC7_UNORM, ColorSpace::Srgb) => vk::Format::BC7_SRGB_BLOCK,
                _ => panic!(
                        "unsupported (image format, color space) pair ({:?}, {:?})",
                        img_format, color_space
                ),
        }
}

pub trait BytesPerPixel {
        fn bytes_per_pixel(&self) -> u32;
}

impl BytesPerPixel for vk::Format {
        fn bytes_per_pixel(&self) -> u32 {
                let f = *self;

                let ranges = [
                        (vk::Format::R8_UNORM, vk::Format::R8_SRGB, 1),
                        (vk::Format::R8G8_UNORM, vk::Format::R8G8_SRGB, 2),
                        (vk::Format::R8G8B8_UNORM, vk::Format::B8G8R8_SRGB, 3),
                        (vk::Format::R8G8B8A8_UNORM, vk::Format::A2B10G10R10_SINT_PACK32, 4),
                        (vk::Format::R16G16B16A16_UNORM, vk::Format::R16G16B16A16_SFLOAT, 8),
                        (vk::Format::R32G32B32A32_UINT, vk::Format::R32G32B32A32_SFLOAT, 16),
                ];

                for range in &ranges {
                        if range.0 <= f && f <= range.1 {
                                return range.2;
                        }
                }

                panic!("unsupported bytes_per_pixel() for format {:?}", f);
        }
}
