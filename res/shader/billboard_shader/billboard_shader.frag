#version 450
#extension GL_EXT_debug_printf : enable

layout (set = 0, binding = 0) uniform WorldMatrices {
        vec4 view_pos;
        mat4 view;
        mat4 proj;
} u_world_matrices;

layout (set = 1, binding = 1) uniform texture2D u_diffuse_map;
layout (set = 1, binding = 3) uniform sampler u_sampler;

layout (location = 0) in vec2 i_tex_coord;

layout (location = 0) out vec4 o_out_color;

void main() {
        vec3 diffuse_texel = texture(sampler2D(u_diffuse_map, u_sampler), i_tex_coord).rgb;
        o_out_color = vec4(diffuse_texel, 1.0);

        float gamma = 2.2;
        o_out_color.rgb = pow(o_out_color.rgb, vec3(1.0 / gamma));
}
