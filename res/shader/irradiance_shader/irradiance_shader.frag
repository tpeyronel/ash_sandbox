#version 450
#extension GL_EXT_debug_printf : enable
#define PI 3.14159265358979323846264338327950288

#resource samplerCube u_environment_map : ENVIRONMENT_MAP;

layout (location = 0) in vec3 i_pos;

layout (location = 0) out vec4 o_output;

void main() {
        const float sample_delta = 0.025;

        vec3 irradiance = vec3(0.0);

        vec3 normal = normalize(i_pos);
        vec3 up = vec3(0.0, 1.0, 0.0);
        vec3 right = normalize(cross(up, normal));
        up = normalize(cross(normal, right));

        float num_samples = 0.0;
        for (float phi = 0.0; phi < 2.0 * PI; phi += sample_delta) {
                for (float theta = 0.0; theta < 0.5 * PI; theta += sample_delta) {
                        // spherical to cartesian (in tangent space)
                        vec3 sample_tangent = vec3(
                                sin(theta) * cos(phi),
                                sin(theta) * sin(phi),
                                cos(theta)
                        );
                        // tangent space to world
                        vec3 sample_world = sample_tangent.x * right + sample_tangent.y * up + sample_tangent.z * normal;

                        irradiance += min(texture(u_environment_map, sample_world).rgb, 50.0) * cos(theta) * sin(theta);
                        num_samples++;
                }
        }
        irradiance = PI * irradiance * (1.0 / float(num_samples));

        o_output = vec4(irradiance, 1.0);
}
