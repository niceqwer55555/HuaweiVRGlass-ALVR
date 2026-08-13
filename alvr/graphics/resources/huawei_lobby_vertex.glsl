#version 300 es

precision highp float;

out vec2 uv;

void main() {
    const vec2 positions[4] = vec2[](
        vec2(-1.0, -1.0),
        vec2(1.0, -1.0),
        vec2(-1.0, 1.0),
        vec2(1.0, 1.0)
    );
    const vec2 texcoords[4] = vec2[](
        vec2(0.0, 1.0),
        vec2(1.0, 1.0),
        vec2(0.0, 0.0),
        vec2(1.0, 0.0)
    );

    gl_Position = vec4(positions[gl_VertexID], 0.0, 1.0);
    // Keep the texture in the same orientation as Huawei's official acquire-bound
    // framebuffer path. The headset optics turn this 90-degree surface into the correct
    // wearer-facing orientation; desktop display captures therefore look rotated.
    uv = texcoords[gl_VertexID];
}
