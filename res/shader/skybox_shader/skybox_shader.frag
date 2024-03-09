#version 450
#extension GL_EXT_debug_printf : enable

#resource ShaderSettings u_settings : SHADER_SETTINGS;
#resource samplerCube u_skybox_texture : SKYBOX;

layout (location = 0) in vec3 i_uvw;

layout (location = 0) out vec4 o_output;

void main() {
        vec3 output_color = textureLod(u_skybox_texture, i_uvw, u_settings.skybox_lod.x).rgb;

        o_output = vec4(output_color, 1.0);
}
