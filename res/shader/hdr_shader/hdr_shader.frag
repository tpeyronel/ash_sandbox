#version 450
#extension GL_EXT_debug_printf : enable

#resource sampler2D u_input_framebuffer : INPUT_FRAMEBUFFER;
#resource ShaderSettings u_shader_settings : SHADER_SETTINGS;

layout (location = 0) in vec2 i_tex_coords;

layout (location = 0) out vec4 o_out_color;

void main() {
        vec4 texel = texture(u_input_framebuffer, i_tex_coords);

        float gamma = u_shader_settings.gamma_and_exposure.x;
        float exposure = u_shader_settings.gamma_and_exposure.y;

        // perform tone mapping
        texel.rgb = vec3(1.0) - exp(-texel.rgb * exposure);

        // perform gamma correction
        o_out_color = vec4(pow(texel.rgb, vec3(1.0 / gamma)), 1.0);
}
