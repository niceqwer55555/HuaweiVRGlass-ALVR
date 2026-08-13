#version 300 es

uniform int view_idx;

out vec2 uv;

void main() {
    vec2 screen_uv = vec2(gl_VertexID & 1, gl_VertexID >> 1);
    gl_Position = vec4((screen_uv - 0.5f) * 2.f, 0, 1);

    // MediaCodec's external texture is vertically inverted in Huawei's legacy
    // acquire-bound framebuffer path. Keep the correction isolated from the
    // standard OpenXR staging renderer used by other headsets.
    uv = vec2((screen_uv.x + float(view_idx)) / 2.f, 1.f - screen_uv.y);
}
