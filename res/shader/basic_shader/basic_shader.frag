#version 450
#extension GL_EXT_debug_printf : enable

layout (set = 0, binding = 0) uniform WorldMatrices {
        vec4 view_pos;
        mat4 view;
        mat4 proj;
} u_world_matrices;

layout (set = 0, binding = 1) uniform WorldLight {
        vec4 pos;
        vec4 color;
} u_light;

layout (set = 1, binding = 0) uniform MaterialData {
        vec4 ambient_color;
        vec4 diffuse_color;
        vec4 specular_color;
        float shininess;
} u_material;
layout (set = 1, binding = 1) uniform texture2D u_texture;
layout (set = 1, binding = 2) uniform sampler u_sampler;


layout (location = 0) in vec3 i_frag_pos;
layout (location = 1) in vec3 i_normal;
layout (location = 2) in vec2 i_tex_coord;

layout (location = 0) out vec4 o_out_color;


void main() {
        float ambient_strength = 0.01;
        vec4 ambient = ambient_strength * u_light.color;

        vec3 normal = normalize(i_normal);
        vec3 light_dir = normalize(i_frag_pos - u_light.pos.xyz);

        float diffuse_angle = max(dot(-light_dir, normal), 0.0);
        vec4 diffuse = diffuse_angle * u_light.color;

        vec3 camera_rdir = normalize(u_world_matrices.view_pos.xyz - i_frag_pos);
        vec3 halfway = normalize(camera_rdir - light_dir);
        float specular_strength = 0.5;
        float specular_angle = max(dot(halfway, normal), 0.0);
        float specular_coefficient = pow(specular_angle, 64.0);
        vec4 specular = (specular_strength * specular_coefficient) * u_light.color;

        vec4 texture = texture(sampler2D(u_texture, u_sampler), i_tex_coord);

        vec4 light_amount = ambient + specular + diffuse;
        o_out_color = light_amount * texture;



        float gamma = 2.2;
        o_out_color.rgb = pow(o_out_color.rgb, vec3(1.0 / gamma));
}
