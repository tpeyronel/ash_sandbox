#version 450
#extension GL_EXT_debug_printf : enable

#resource WorldMatrices u_world_matrices : WORLD_MATRICES;
#resource WorldLights u_world_lights : WORLD_LIGHTS;

layout (location = 0) in vec3 i_frag_pos; // The fragment position in world space

layout (location = 0) out float o_depth;

void main() {
        float light_distance = length(i_frag_pos - u_world_lights.point_light.pos.xyz);

        // // map to [0;1] range by dividing by far_plane
        // light_distance = light_distance / 20.0;

        o_depth = light_distance;
}
