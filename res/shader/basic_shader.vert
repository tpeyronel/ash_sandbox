#version 450

layout (set = 0, binding = 0) uniform Matrices {
        mat4 model;
        mat4 view;
        mat4 proj;
} u_mats;


layout (location = 0) in vec3 i_pos;
layout (location = 1) in vec2 i_tex_coord;


layout (location = 0) out vec2 o_tex_coord;



void main() {
        gl_Position = u_mats.proj * u_mats.view * u_mats.model * vec4(i_pos, 1.0);
        //gl_Position = u_mats.model * vec4(i_pos, 1.0);
        //gl_Position = vec4(i_pos, 1.0);

        o_tex_coord = i_tex_coord;
}

/*#version 450
#extension GL_ARB_separate_shader_objects : enable

out gl_PerVertex {
    vec4 gl_Position;
};

layout(location = 0) out vec3 o_frag_color;

vec2 positions[3] = vec2[](
    vec2(0.0, -0.5),
    vec2(0.5, 0.5),
    vec2(-0.5, 0.5)
);

vec3 colors[3] = vec3[](
    vec3(1.0, 0.0, 0.0),
    vec3(0.0, 1.0, 0.0),
    vec3(0.0, 0.0, 1.0)
);

void main() {
    gl_Position = vec4(positions[gl_VertexIndex], 0.0, 1.0);
    o_frag_color = colors[gl_VertexIndex];
}*/