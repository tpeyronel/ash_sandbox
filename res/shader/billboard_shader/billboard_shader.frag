#version 450
#extension GL_EXT_debug_printf : enable

#resource WorldMatrices u_world_matrices : WORLD_MATRICES;
#resource sampler2D u_diffuse_map : MATERIAL_DIFFUSE_TEXTURE;

layout (location = 0) in vec2 i_tex_coord;

layout (location = 0) out vec4 o_out_color;

void main() {
        vec3 diffuse_texel = texture(u_diffuse_map, i_tex_coord).rgb;
        o_out_color = vec4(diffuse_texel, 1.0);

        float gamma = 2.2;
        o_out_color.rgb = pow(o_out_color.rgb, vec3(1.0 / gamma));
}
