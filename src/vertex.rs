use std::mem::size_of;

use ash::{vk, vk::VertexInputRate};

use crate::my_glm::*;

pub struct Vertex {
        pub pos: Vec3,
        //pub tex_coord: Vec2,
}

impl Vertex {
        pub fn vk_binding_description() -> [vk::VertexInputBindingDescription; 2] {
                [
                        vk::VertexInputBindingDescription {
                                binding: 0,
                                stride: size_of::<Vec3>() as u32,
                                input_rate: VertexInputRate::VERTEX,
                        },
                        vk::VertexInputBindingDescription {
                                binding: 1,
                                stride: size_of::<Vec2>() as u32,
                                input_rate: VertexInputRate::VERTEX,
                        },
                ]
        }

        pub fn vk_attribute_descriptions() -> [vk::VertexInputAttributeDescription; 2] {
                [
                        vk::VertexInputAttributeDescription {
                                location: 0,
                                binding: 0,
                                format: vk::Format::R32G32B32_SFLOAT,
                                offset: 0,
                                //offset: memoffset::offset_of!(Vertex, pos) as u32,
                        },
                        vk::VertexInputAttributeDescription {
                                location: 1,
                                binding: 1,
                                format: vk::Format::R32G32_SFLOAT,
                                offset: 0,
                                //offset:   memoffset::offset_of!(Vertex, tex_coord) as u32,
                        },
                ]
        }
}
