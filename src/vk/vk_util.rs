use ash::{prelude::VkResult, vk};

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
