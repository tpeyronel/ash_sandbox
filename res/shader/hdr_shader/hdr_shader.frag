#version 450
#extension GL_EXT_debug_printf : enable

#resource sampler2D u_input_framebuffer : INPUT_FRAMEBUFFER;

layout (location = 0) in vec2 i_tex_coords;

layout (location = 0) out vec4 o_out_color;

void main() {
        vec4 texel = texture(u_input_framebuffer, i_tex_coords);

        // texel.rgb = texel.rgb / (texel.rgb + 1.0);

        float gamma = 2.2;

        o_out_color = vec4(pow(texel.rgb, vec3(1.0 / gamma)), 1.0);
}
