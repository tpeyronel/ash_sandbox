use ash::vk;

pub trait ImageSubresourceRangeUtil {
        fn full(aspect_mask: vk::ImageAspectFlags) -> Self;
        fn full_color() -> Self;
        fn full_depth() -> Self;
}

impl ImageSubresourceRangeUtil for vk::ImageSubresourceRange {
        fn full(aspect_mask: vk::ImageAspectFlags) -> Self {
                Self {
                        aspect_mask,
                        base_mip_level: 0,
                        level_count: vk::REMAINING_MIP_LEVELS,
                        base_array_layer: 0,
                        layer_count: vk::REMAINING_ARRAY_LAYERS,
                }
        }

        fn full_color() -> Self {
                Self::full(vk::ImageAspectFlags::COLOR)
        }

        fn full_depth() -> Self {
                Self::full(vk::ImageAspectFlags::DEPTH)
        }
}
