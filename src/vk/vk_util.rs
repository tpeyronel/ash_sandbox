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
                _ => panic!(
                        "unsupported (image format, color space) pair ({:?}, {:?})",
                        img_format, color_space
                ),
        }
}
