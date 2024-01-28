pub type ShaderResourceId = String;

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
                                        .map(|f| format!("\t{} {};", f.field_type.glsl_type_name(), f.field_name))
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
                self.fields
                        .iter()
                        .map(|f| match &f.field_type {
                                ShaderStructFieldType::Struct(child) => child.compute_size(),
                                ShaderStructFieldType::Vec2 => std::mem::size_of::<crate::my_glm::Vec2>(),
                                ShaderStructFieldType::Vec4 => std::mem::size_of::<crate::my_glm::Vec4>(),
                                ShaderStructFieldType::Mat4 => std::mem::size_of::<crate::my_glm::Mat4>(),
                        })
                        .sum()
        }

        pub fn glsl_type_declaration(&self) -> String {
                let fields = self
                        .fields
                        .iter()
                        .map(|f| format!("\t{} {};", f.field_type.glsl_type_name(), f.field_name))
                        .collect::<Vec<String>>()
                        .join("\n");

                format!("struct {} {{\n{}\n}};\n\n", self.type_name, fields)
        }
}

pub trait ShaderStruct {
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
}

impl ShaderStructFieldType {
        pub fn glsl_type_name(&self) -> &str {
                match self {
                        ShaderStructFieldType::Vec2 => "vec2",
                        ShaderStructFieldType::Vec4 => "vec4",
                        ShaderStructFieldType::Mat4 => "mat4",
                        ShaderStructFieldType::Struct(ShaderStructDeclaration { type_name, .. }) => &type_name,
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

impl<T> ShaderStructFieldTypeProvider for T
where
        T: ShaderStruct,
{
        fn shader_struct_field_type() -> ShaderStructFieldType {
                ShaderStructFieldType::Struct(Self::shader_struct_declaration())
        }
}
