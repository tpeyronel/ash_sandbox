#version 450

layout (set = 0, binding = 0) uniform Matrices_V_P {
        mat4 view;
        mat4 proj;
} u_mats_v_p;

layout (push_constant) uniform Matrices_M_MVP {
        mat4 model;
        mat4 mvp;
} u_mats_m_mvp;

layout (location = 0) in vec3 i_pos;
layout (location = 1) in vec4 i_color;

layout (location = 0) out vec4 o_color;

void main() {
        o_color = i_color;

        gl_Position = u_mats_m_mvp.mvp * vec4(i_pos, 1.0);
}