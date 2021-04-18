#version 450

layout (set = 1, binding = 0) uniform texture2D u_texture;
layout (set = 1, binding = 1) uniform sampler u_sampler;

layout (location = 0) in vec2 i_tex_coord;

layout (location = 0) out vec4 o_out_color;

void main() {
        o_out_color = texture(sampler2D(u_texture, u_sampler), i_tex_coord);

        float gamma = 2.2;
        o_out_color.rgb = pow(o_out_color.rgb, vec3(1.0 / gamma));

        //o_out_color = vec4(i_tex_coord, 0.0, 1.0);
        //o_out_color = vec4(1.0, 1.0, 1.0, 1.0);
}
