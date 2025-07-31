use std::{fmt::Display, ops::Deref, sync::Arc};

use bytemuck::NoUninit;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShaderResourceId(Arc<str>);

impl ShaderResourceId {
        pub fn new(id: &str) -> Self {
                Self(Arc::from(id))
        }
}

impl From<&str> for ShaderResourceId {
        fn from(value: &str) -> Self {
                Self::new(value)
        }
}

impl Deref for ShaderResourceId {
        type Target = str;

        fn deref(&self) -> &Self::Target {
                &self.0
        }
}

impl Display for ShaderResourceId {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                Display::fmt(&self.0, f)
        }
}

#[derive(Debug, Clone)]
pub struct ShaderResource {
        pub id: ShaderResourceId,
        pub resource_type: ShaderResourceType,
        pub provider: ShaderResourceProvider,
}

#[derive(Debug, Clone)]
pub enum ShaderResourceType {
        Struct(ShaderStructDeclaration),
        Image2D,
        ImageCube,
}

impl ShaderResourceType {
        pub fn glsl_type_name<'a>(&'a self) -> &'a str {
                match self {
                        ShaderResourceType::Struct(ShaderStructDeclaration { type_name, .. }) => &type_name,
                        ShaderResourceType::Image2D => "sampler2D",
                        ShaderResourceType::ImageCube => "samplerCube",
                }
        }

        pub fn glsl_complete_type(&self) -> String {
                match self {
                        ShaderResourceType::Struct(ShaderStructDeclaration { type_name, fields }) => {
                                let body: String = fields
                                        .iter()
                                        .map(|f| {
                                                format!("\t{};", f.field_type.glsl_type_name_with_field(&f.field_name))
                                        })
                                        .collect::<Vec<String>>()
                                        .join("\n");

                                format!("{} {{\n{}\n}}", type_name, body)
                        },
                        _ => self.glsl_type_name().to_owned(),
                }
        }
}

#[derive(Debug, Hash, Clone, Copy)]
pub enum ShaderResourceProvider {
        World,
        RenderPass,
        Material,
        Mesh,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderStructDeclaration {
        pub type_name: String,
        pub fields: Vec<ShaderStructField>,
}

impl ShaderStructDeclaration {
        pub fn compute_size(&self) -> usize {
                self.fields.iter().map(|f| f.field_type.compute_size()).sum()
        }

        pub fn glsl_type_declaration(&self) -> String {
                let fields = self
                        .fields
                        .iter()
                        .map(|f| format!("\t{};", f.field_type.glsl_type_name_with_field(&f.field_name)))
                        .collect::<Vec<String>>()
                        .join("\n");

                format!("struct {} {{\n{}\n}};\n\n", self.type_name, fields)
        }
}

pub trait ShaderStruct: NoUninit {
        fn shader_struct_declaration() -> ShaderStructDeclaration;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaderStructField {
        pub field_name: String,
        pub field_type: ShaderStructFieldType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShaderStructFieldType {
        Struct(ShaderStructDeclaration),
        Vec2,
        Vec4,
        Mat4,
        Vec2u,
        Vec4u,
        Array {
                element_type: Box<ShaderStructFieldType>,
                length: usize,
        },
}

impl ShaderStructFieldType {
        fn compute_size(&self) -> usize {
                match &self {
                        ShaderStructFieldType::Struct(declaration) => declaration.compute_size(),
                        ShaderStructFieldType::Vec2 => std::mem::size_of::<crate::my_glm::Vec2>(),
                        ShaderStructFieldType::Vec4 => std::mem::size_of::<crate::my_glm::Vec4>(),
                        ShaderStructFieldType::Mat4 => std::mem::size_of::<crate::my_glm::Mat4>(),
                        ShaderStructFieldType::Vec2u => std::mem::size_of::<crate::my_glm::Vec2u>(),
                        ShaderStructFieldType::Vec4u => std::mem::size_of::<crate::my_glm::Vec4u>(),
                        ShaderStructFieldType::Array { element_type, length } => element_type.compute_size() * length,
                }
        }

        pub fn glsl_type_name(&self) -> &str {
                match self {
                        ShaderStructFieldType::Vec2 => "vec2",
                        ShaderStructFieldType::Vec4 => "vec4",
                        ShaderStructFieldType::Mat4 => "mat4",
                        ShaderStructFieldType::Vec2u => "uvec2",
                        ShaderStructFieldType::Vec4u => "uvec4",
                        ShaderStructFieldType::Struct(ShaderStructDeclaration { type_name, .. }) => &type_name,
                        ShaderStructFieldType::Array { element_type, .. } => element_type.glsl_type_name(),
                }
        }

        pub fn glsl_type_name_with_field(&self, field_name: &str) -> String {
                match self {
                        ShaderStructFieldType::Array { element_type, length } => {
                                format!("{} {}[{}]", element_type.glsl_type_name(), field_name, length)
                        },
                        _ => format!("{} {}", self.glsl_type_name(), field_name),
                }
        }
}

pub trait ShaderStructFieldTypeProvider {
        fn shader_struct_field_type() -> ShaderStructFieldType;
}

impl ShaderStructFieldTypeProvider for crate::my_glm::Vec2 {
        fn shader_struct_field_type() -> ShaderStructFieldType {
                ShaderStructFieldType::Vec2
        }
}

impl ShaderStructFieldTypeProvider for crate::my_glm::Vec4 {
        fn shader_struct_field_type() -> ShaderStructFieldType {
                ShaderStructFieldType::Vec4
        }
}

impl ShaderStructFieldTypeProvider for crate::my_glm::Mat4 {
        fn shader_struct_field_type() -> ShaderStructFieldType {
                ShaderStructFieldType::Mat4
        }
}

impl ShaderStructFieldTypeProvider for crate::my_glm::Vec2u {
        fn shader_struct_field_type() -> ShaderStructFieldType {
                ShaderStructFieldType::Vec2u
        }
}

impl ShaderStructFieldTypeProvider for crate::my_glm::Vec4u {
        fn shader_struct_field_type() -> ShaderStructFieldType {
                ShaderStructFieldType::Vec4u
        }
}

impl<T> ShaderStructFieldTypeProvider for T
where
        T: ShaderStruct,
{
        fn shader_struct_field_type() -> ShaderStructFieldType {
                ShaderStructFieldType::Struct(Self::shader_struct_declaration())
        }
}

impl<T, const N: usize> ShaderStructFieldTypeProvider for [T; N]
where
        T: ShaderStructFieldTypeProvider,
{
        fn shader_struct_field_type() -> ShaderStructFieldType {
                ShaderStructFieldType::Array {
                        element_type: Box::new(T::shader_struct_field_type()),
                        length: N,
                }
        }
}
