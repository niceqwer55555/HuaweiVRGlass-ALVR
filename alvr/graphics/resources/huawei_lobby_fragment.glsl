#version 300 es

precision mediump float;

uniform sampler2D lobby_texture;
in vec2 uv;
out vec4 color;

void main() {
    color = texture(lobby_texture, uv);
}
