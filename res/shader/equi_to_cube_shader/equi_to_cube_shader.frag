#version 450
#extension GL_EXT_debug_printf : enable
#define PI 3.14159265358979323846264338327950288

#resource sampler2D u_equirectangular_map : EQUIRECTANGULAR_MAP;

layout (location = 0) in vec3 i_pos;

layout (location = 0) out vec4 o_output;

const vec2 inv_atan = vec2(1.0 / (2.0 * PI), 1.0 / PI);
vec2 sample_equirectangular_map(vec3 pos) {
        vec2 uv = vec2(atan(pos.z, pos.x), asin(-pos.y));
        uv *= inv_atan;
        uv += 0.5;
        uv.x = 1.0 - uv.x;
        return uv;
}

void main() {
        vec2 uv = sample_equirectangular_map(normalize(i_pos));
        vec3 color = texture(u_equirectangular_map, uv).rgb;

        o_output = vec4(color, 1.0);
}
