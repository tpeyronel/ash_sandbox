#version 450

layout (set = 0, binding = 1) uniform sampler2D u_tex;

layout(location = 0) in vec2 i_tex_coord;

layout(location = 0) out vec4 o_out_color;

void main() {
        o_out_color = texture(u_tex, i_tex_coord);
        //o_out_color = vec4(0.1, 0.1, 0.1, 1.0);
//
        float gamma = 2.2;
        o_out_color.rgb = pow(o_out_color.rgb, vec3(1.0/gamma));

        //o_out_color = vec4(i_tex_coord, 0.0, 1.0);
}

/*#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(location = 0) in vec3 i_frag_color;

layout(location = 0) out vec4 o_out_color;

void main() {
    o_out_color = vec4(i_frag_color, 1.0);
}*/