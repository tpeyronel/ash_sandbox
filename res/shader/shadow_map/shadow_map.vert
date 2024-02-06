#version 450
#extension GL_EXT_debug_printf : enable

#resource WorldLights u_world_lights : WORLD_LIGHTS;
#resource ObjectMatrices u_object_matrices : OBJECT_MATRICES;

layout (location = 0) in vec3 i_pos;

void main() {
        gl_Position = u_world_lights.dir_light.vp * u_object_matrices.model * vec4(i_pos, 1.0);
}