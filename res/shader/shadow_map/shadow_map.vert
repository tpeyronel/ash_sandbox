#version 450
#extension GL_EXT_debug_printf : enable

#resource ObjectMatrices u_object_matrices : OBJECT_MATRICES;

layout (location = 0) in vec3 i_pos;

void main() {
        gl_Position = u_object_matrices.mvp * vec4(i_pos, 1.0);
}