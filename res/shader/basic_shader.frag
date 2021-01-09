#version 450

layout (set = 0, binding = 1) uniform sampler2D u_tex;

layout(location = 0) in vec2 i_tex_coord;

layout(location = 0) out vec4 o_out_color;

void main() {
        o_out_color = texture(u_tex, i_tex_coord);
        //o_out_color = vec4(0.0, 1.0, 0.0, 1.0);
}

/*#version 450
#extension GL_ARB_separate_shader_objects : enable

layout(location = 0) in vec3 fragColor;

layout(location = 0) out vec4 outColor;

void main() {
    outColor = vec4(fragColor, 1.0);
}*/