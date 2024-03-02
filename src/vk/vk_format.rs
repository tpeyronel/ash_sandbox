use ash::vk;

use crate::util;

pub trait VkFormatProperties {
        fn block_size(&self) -> u32;
        fn block_extent(&self) -> vk::Extent3D;

        fn compute_stride(&self, width: u32, height: u32) -> u32 {
                let extent = self.block_extent();

                util::compute_image_stride(width, height, self.block_size(), (extent.width, extent.height))
        }

        fn compute_stride_with_mipmaps(&self, width: u32, height: u32, mipmaps: u32) -> u32 {
                let extent = self.block_extent();

                util::compute_image_stride_with_mipmaps(
                        width,
                        height,
                        self.block_size(),
                        (extent.width, extent.height),
                        mipmaps,
                )
        }
}

impl VkFormatProperties for vk::Format {
        fn block_size(&self) -> u32 {
                match *self {
                        vk::Format::R4G4_UNORM_PACK8
                        | vk::Format::R8_UNORM
                        | vk::Format::R8_SNORM
                        | vk::Format::R8_USCALED
                        | vk::Format::R8_SSCALED
                        | vk::Format::R8_UINT
                        | vk::Format::R8_SINT
                        | vk::Format::R8_SRGB => 1,

                        vk::Format::R10X6_UNORM_PACK16
                        | vk::Format::R12X4_UNORM_PACK16
                        | vk::Format::A4R4G4B4_UNORM_PACK16
                        | vk::Format::A4B4G4R4_UNORM_PACK16
                        | vk::Format::R4G4B4A4_UNORM_PACK16
                        | vk::Format::B4G4R4A4_UNORM_PACK16
                        | vk::Format::R5G6B5_UNORM_PACK16
                        | vk::Format::B5G6R5_UNORM_PACK16
                        | vk::Format::R5G5B5A1_UNORM_PACK16
                        | vk::Format::B5G5R5A1_UNORM_PACK16
                        | vk::Format::A1R5G5B5_UNORM_PACK16
                        | vk::Format::R8G8_UNORM
                        | vk::Format::R8G8_SNORM
                        | vk::Format::R8G8_USCALED
                        | vk::Format::R8G8_SSCALED
                        | vk::Format::R8G8_UINT
                        | vk::Format::R8G8_SINT
                        | vk::Format::R8G8_SRGB
                        | vk::Format::R16_UNORM
                        | vk::Format::R16_SNORM
                        | vk::Format::R16_USCALED
                        | vk::Format::R16_SSCALED
                        | vk::Format::R16_UINT
                        | vk::Format::R16_SINT
                        | vk::Format::R16_SFLOAT => 2,

                        vk::Format::R8G8B8_UNORM
                        | vk::Format::R8G8B8_SNORM
                        | vk::Format::R8G8B8_USCALED
                        | vk::Format::R8G8B8_SSCALED
                        | vk::Format::R8G8B8_UINT
                        | vk::Format::R8G8B8_SINT
                        | vk::Format::R8G8B8_SRGB
                        | vk::Format::B8G8R8_UNORM
                        | vk::Format::B8G8R8_SNORM
                        | vk::Format::B8G8R8_USCALED
                        | vk::Format::B8G8R8_SSCALED
                        | vk::Format::B8G8R8_UINT
                        | vk::Format::B8G8R8_SINT
                        | vk::Format::B8G8R8_SRGB => 3,

                        vk::Format::R10X6G10X6_UNORM_2PACK16
                        | vk::Format::R12X4G12X4_UNORM_2PACK16
                        | vk::Format::R16G16_S10_5_NV
                        | vk::Format::R8G8B8A8_UNORM
                        | vk::Format::R8G8B8A8_SNORM
                        | vk::Format::R8G8B8A8_USCALED
                        | vk::Format::R8G8B8A8_SSCALED
                        | vk::Format::R8G8B8A8_UINT
                        | vk::Format::R8G8B8A8_SINT
                        | vk::Format::R8G8B8A8_SRGB
                        | vk::Format::B8G8R8A8_UNORM
                        | vk::Format::B8G8R8A8_SNORM
                        | vk::Format::B8G8R8A8_USCALED
                        | vk::Format::B8G8R8A8_SSCALED
                        | vk::Format::B8G8R8A8_UINT
                        | vk::Format::B8G8R8A8_SINT
                        | vk::Format::B8G8R8A8_SRGB
                        | vk::Format::A8B8G8R8_UNORM_PACK32
                        | vk::Format::A8B8G8R8_SNORM_PACK32
                        | vk::Format::A8B8G8R8_USCALED_PACK32
                        | vk::Format::A8B8G8R8_SSCALED_PACK32
                        | vk::Format::A8B8G8R8_UINT_PACK32
                        | vk::Format::A8B8G8R8_SINT_PACK32
                        | vk::Format::A8B8G8R8_SRGB_PACK32
                        | vk::Format::A2R10G10B10_UNORM_PACK32
                        | vk::Format::A2R10G10B10_SNORM_PACK32
                        | vk::Format::A2R10G10B10_USCALED_PACK32
                        | vk::Format::A2R10G10B10_SSCALED_PACK32
                        | vk::Format::A2R10G10B10_UINT_PACK32
                        | vk::Format::A2R10G10B10_SINT_PACK32
                        | vk::Format::A2B10G10R10_UNORM_PACK32
                        | vk::Format::A2B10G10R10_SNORM_PACK32
                        | vk::Format::A2B10G10R10_USCALED_PACK32
                        | vk::Format::A2B10G10R10_SSCALED_PACK32
                        | vk::Format::A2B10G10R10_UINT_PACK32
                        | vk::Format::A2B10G10R10_SINT_PACK32
                        | vk::Format::R16G16_UNORM
                        | vk::Format::R16G16_SNORM
                        | vk::Format::R16G16_USCALED
                        | vk::Format::R16G16_SSCALED
                        | vk::Format::R16G16_UINT
                        | vk::Format::R16G16_SINT
                        | vk::Format::R16G16_SFLOAT
                        | vk::Format::R32_UINT
                        | vk::Format::R32_SINT
                        | vk::Format::R32_SFLOAT
                        | vk::Format::B10G11R11_UFLOAT_PACK32
                        | vk::Format::E5B9G9R9_UFLOAT_PACK32 => 4,

                        vk::Format::R16G16B16_UNORM
                        | vk::Format::R16G16B16_SNORM
                        | vk::Format::R16G16B16_USCALED
                        | vk::Format::R16G16B16_SSCALED
                        | vk::Format::R16G16B16_UINT
                        | vk::Format::R16G16B16_SINT
                        | vk::Format::R16G16B16_SFLOAT => 6,

                        vk::Format::R16G16B16A16_UNORM
                        | vk::Format::R16G16B16A16_SNORM
                        | vk::Format::R16G16B16A16_USCALED
                        | vk::Format::R16G16B16A16_SSCALED
                        | vk::Format::R16G16B16A16_UINT
                        | vk::Format::R16G16B16A16_SINT
                        | vk::Format::R16G16B16A16_SFLOAT
                        | vk::Format::R32G32_UINT
                        | vk::Format::R32G32_SINT
                        | vk::Format::R32G32_SFLOAT
                        | vk::Format::R64_UINT
                        | vk::Format::R64_SINT
                        | vk::Format::R64_SFLOAT => 8,

                        vk::Format::R32G32B32_UINT | vk::Format::R32G32B32_SINT | vk::Format::R32G32B32_SFLOAT => 12,

                        vk::Format::R32G32B32A32_UINT
                        | vk::Format::R32G32B32A32_SINT
                        | vk::Format::R32G32B32A32_SFLOAT
                        | vk::Format::R64G64_UINT
                        | vk::Format::R64G64_SINT
                        | vk::Format::R64G64_SFLOAT => 16,

                        vk::Format::R64G64B64_UINT | vk::Format::R64G64B64_SINT | vk::Format::R64G64B64_SFLOAT => 24,

                        vk::Format::R64G64B64A64_UINT
                        | vk::Format::R64G64B64A64_SINT
                        | vk::Format::R64G64B64A64_SFLOAT => 32,

                        vk::Format::BC1_RGB_UNORM_BLOCK | vk::Format::BC1_RGB_SRGB_BLOCK => 8,
                        vk::Format::BC1_RGBA_UNORM_BLOCK | vk::Format::BC1_RGBA_SRGB_BLOCK => 8,
                        vk::Format::BC2_UNORM_BLOCK | vk::Format::BC2_SRGB_BLOCK => 16,
                        vk::Format::BC3_UNORM_BLOCK | vk::Format::BC3_SRGB_BLOCK => 16,
                        vk::Format::BC4_UNORM_BLOCK | vk::Format::BC4_SNORM_BLOCK => 8,
                        vk::Format::BC5_UNORM_BLOCK | vk::Format::BC5_SNORM_BLOCK => 16,
                        vk::Format::BC6H_UFLOAT_BLOCK | vk::Format::BC6H_SFLOAT_BLOCK => 16,
                        vk::Format::BC7_UNORM_BLOCK | vk::Format::BC7_SRGB_BLOCK => 16,

                        _ => panic!("unsupported format {:?}", *self),
                }
        }

        fn block_extent(&self) -> vk::Extent3D {
                let (width, height, depth) = match *self {
                        vk::Format::R4G4_UNORM_PACK8
                        | vk::Format::R8_UNORM
                        | vk::Format::R8_SNORM
                        | vk::Format::R8_USCALED
                        | vk::Format::R8_SSCALED
                        | vk::Format::R8_UINT
                        | vk::Format::R8_SINT
                        | vk::Format::R8_SRGB => (1, 1, 1),

                        vk::Format::R10X6_UNORM_PACK16
                        | vk::Format::R12X4_UNORM_PACK16
                        | vk::Format::A4R4G4B4_UNORM_PACK16
                        | vk::Format::A4B4G4R4_UNORM_PACK16
                        | vk::Format::R4G4B4A4_UNORM_PACK16
                        | vk::Format::B4G4R4A4_UNORM_PACK16
                        | vk::Format::R5G6B5_UNORM_PACK16
                        | vk::Format::B5G6R5_UNORM_PACK16
                        | vk::Format::R5G5B5A1_UNORM_PACK16
                        | vk::Format::B5G5R5A1_UNORM_PACK16
                        | vk::Format::A1R5G5B5_UNORM_PACK16
                        | vk::Format::R8G8_UNORM
                        | vk::Format::R8G8_SNORM
                        | vk::Format::R8G8_USCALED
                        | vk::Format::R8G8_SSCALED
                        | vk::Format::R8G8_UINT
                        | vk::Format::R8G8_SINT
                        | vk::Format::R8G8_SRGB
                        | vk::Format::R16_UNORM
                        | vk::Format::R16_SNORM
                        | vk::Format::R16_USCALED
                        | vk::Format::R16_SSCALED
                        | vk::Format::R16_UINT
                        | vk::Format::R16_SINT
                        | vk::Format::R16_SFLOAT => (1, 1, 1),

                        vk::Format::R8G8B8_UNORM
                        | vk::Format::R8G8B8_SNORM
                        | vk::Format::R8G8B8_USCALED
                        | vk::Format::R8G8B8_SSCALED
                        | vk::Format::R8G8B8_UINT
                        | vk::Format::R8G8B8_SINT
                        | vk::Format::R8G8B8_SRGB
                        | vk::Format::B8G8R8_UNORM
                        | vk::Format::B8G8R8_SNORM
                        | vk::Format::B8G8R8_USCALED
                        | vk::Format::B8G8R8_SSCALED
                        | vk::Format::B8G8R8_UINT
                        | vk::Format::B8G8R8_SINT
                        | vk::Format::B8G8R8_SRGB => (1, 1, 1),

                        vk::Format::R10X6G10X6_UNORM_2PACK16
                        | vk::Format::R12X4G12X4_UNORM_2PACK16
                        | vk::Format::R16G16_S10_5_NV
                        | vk::Format::R8G8B8A8_UNORM
                        | vk::Format::R8G8B8A8_SNORM
                        | vk::Format::R8G8B8A8_USCALED
                        | vk::Format::R8G8B8A8_SSCALED
                        | vk::Format::R8G8B8A8_UINT
                        | vk::Format::R8G8B8A8_SINT
                        | vk::Format::R8G8B8A8_SRGB
                        | vk::Format::B8G8R8A8_UNORM
                        | vk::Format::B8G8R8A8_SNORM
                        | vk::Format::B8G8R8A8_USCALED
                        | vk::Format::B8G8R8A8_SSCALED
                        | vk::Format::B8G8R8A8_UINT
                        | vk::Format::B8G8R8A8_SINT
                        | vk::Format::B8G8R8A8_SRGB
                        | vk::Format::A8B8G8R8_UNORM_PACK32
                        | vk::Format::A8B8G8R8_SNORM_PACK32
                        | vk::Format::A8B8G8R8_USCALED_PACK32
                        | vk::Format::A8B8G8R8_SSCALED_PACK32
                        | vk::Format::A8B8G8R8_UINT_PACK32
                        | vk::Format::A8B8G8R8_SINT_PACK32
                        | vk::Format::A8B8G8R8_SRGB_PACK32
                        | vk::Format::A2R10G10B10_UNORM_PACK32
                        | vk::Format::A2R10G10B10_SNORM_PACK32
                        | vk::Format::A2R10G10B10_USCALED_PACK32
                        | vk::Format::A2R10G10B10_SSCALED_PACK32
                        | vk::Format::A2R10G10B10_UINT_PACK32
                        | vk::Format::A2R10G10B10_SINT_PACK32
                        | vk::Format::A2B10G10R10_UNORM_PACK32
                        | vk::Format::A2B10G10R10_SNORM_PACK32
                        | vk::Format::A2B10G10R10_USCALED_PACK32
                        | vk::Format::A2B10G10R10_SSCALED_PACK32
                        | vk::Format::A2B10G10R10_UINT_PACK32
                        | vk::Format::A2B10G10R10_SINT_PACK32
                        | vk::Format::R16G16_UNORM
                        | vk::Format::R16G16_SNORM
                        | vk::Format::R16G16_USCALED
                        | vk::Format::R16G16_SSCALED
                        | vk::Format::R16G16_UINT
                        | vk::Format::R16G16_SINT
                        | vk::Format::R16G16_SFLOAT
                        | vk::Format::R32_UINT
                        | vk::Format::R32_SINT
                        | vk::Format::R32_SFLOAT
                        | vk::Format::B10G11R11_UFLOAT_PACK32
                        | vk::Format::E5B9G9R9_UFLOAT_PACK32 => (1, 1, 1),

                        vk::Format::R16G16B16_UNORM
                        | vk::Format::R16G16B16_SNORM
                        | vk::Format::R16G16B16_USCALED
                        | vk::Format::R16G16B16_SSCALED
                        | vk::Format::R16G16B16_UINT
                        | vk::Format::R16G16B16_SINT
                        | vk::Format::R16G16B16_SFLOAT => (1, 1, 1),

                        vk::Format::R16G16B16A16_UNORM
                        | vk::Format::R16G16B16A16_SNORM
                        | vk::Format::R16G16B16A16_USCALED
                        | vk::Format::R16G16B16A16_SSCALED
                        | vk::Format::R16G16B16A16_UINT
                        | vk::Format::R16G16B16A16_SINT
                        | vk::Format::R16G16B16A16_SFLOAT
                        | vk::Format::R32G32_UINT
                        | vk::Format::R32G32_SINT
                        | vk::Format::R32G32_SFLOAT
                        | vk::Format::R64_UINT
                        | vk::Format::R64_SINT
                        | vk::Format::R64_SFLOAT => (1, 1, 1),

                        vk::Format::R32G32B32_UINT | vk::Format::R32G32B32_SINT | vk::Format::R32G32B32_SFLOAT => {
                                (1, 1, 1)
                        },

                        vk::Format::R32G32B32A32_UINT
                        | vk::Format::R32G32B32A32_SINT
                        | vk::Format::R32G32B32A32_SFLOAT
                        | vk::Format::R64G64_UINT
                        | vk::Format::R64G64_SINT
                        | vk::Format::R64G64_SFLOAT => (1, 1, 1),

                        vk::Format::R64G64B64_UINT | vk::Format::R64G64B64_SINT | vk::Format::R64G64B64_SFLOAT => {
                                (1, 1, 1)
                        },

                        vk::Format::R64G64B64A64_UINT
                        | vk::Format::R64G64B64A64_SINT
                        | vk::Format::R64G64B64A64_SFLOAT => (1, 1, 1),

                        vk::Format::BC1_RGB_UNORM_BLOCK | vk::Format::BC1_RGB_SRGB_BLOCK => (4, 4, 1),
                        vk::Format::BC1_RGBA_UNORM_BLOCK | vk::Format::BC1_RGBA_SRGB_BLOCK => (4, 4, 1),
                        vk::Format::BC2_UNORM_BLOCK | vk::Format::BC2_SRGB_BLOCK => (4, 4, 1),
                        vk::Format::BC3_UNORM_BLOCK | vk::Format::BC3_SRGB_BLOCK => (4, 4, 1),
                        vk::Format::BC4_UNORM_BLOCK | vk::Format::BC4_SNORM_BLOCK => (4, 4, 1),
                        vk::Format::BC5_UNORM_BLOCK | vk::Format::BC5_SNORM_BLOCK => (4, 4, 1),
                        vk::Format::BC6H_UFLOAT_BLOCK | vk::Format::BC6H_SFLOAT_BLOCK => (4, 4, 1),
                        vk::Format::BC7_UNORM_BLOCK | vk::Format::BC7_SRGB_BLOCK => (4, 4, 1),

                        _ => panic!("unsupported format {:?}", *self),
                };

                vk::Extent3D { width, height, depth }
        }
}
