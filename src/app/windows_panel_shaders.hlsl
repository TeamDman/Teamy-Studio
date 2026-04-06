struct VsInput {
    float3 position : POSITION;
    float4 color : COLOR;
    float2 uv : TEXCOORD;
    float effect : EFFECT;
    float glyph : GLYPH;
};

struct PsInput {
    float4 position : SV_POSITION;
    float4 color : COLOR;
    float2 uv : TEXCOORD;
    float effect : EFFECT;
    float glyph : GLYPH;
};

Buffer<uint> GlyphRows : register(t0);

static const int FONT_ATLAS_COLUMNS = 16;
static const int FONT_ATLAS_CELL_WIDTH = 32;
static const int FONT_ATLAS_CELL_HEIGHT = 64;
static const int FONT_ATLAS_WIDTH = FONT_ATLAS_COLUMNS * FONT_ATLAS_CELL_WIDTH;

PsInput VSMain(VsInput input) {
    PsInput output;
    output.position = float4(input.position, 1.0);
    output.color = input.color;
    output.uv = input.uv;
    output.effect = input.effect;
    output.glyph = input.glyph;
    return output;
}

float sdBox(float2 p, float2 b) {
    float2 d = abs(p) - b;
    return length(max(d, 0.0)) + min(max(d.x, d.y), 0.0);
}

float cross2d(float2 a, float2 b) {
    return (a.x * b.y) - (a.y * b.x);
}

float segment_distance(float2 p, float2 a, float2 b) {
    float2 pa = p - a;
    float2 ba = b - a;
    float h = saturate(dot(pa, ba) / dot(ba, ba));
    return length(pa - (ba * h));
}

float triangle_mask(float2 p, float2 a, float2 b, float2 c, float thickness) {
    float s0 = cross2d(b - a, p - a);
    float s1 = cross2d(c - b, p - b);
    float s2 = cross2d(a - c, p - c);
    bool inside = ((s0 >= 0.0) && (s1 >= 0.0) && (s2 >= 0.0)) || ((s0 <= 0.0) && (s1 <= 0.0) && (s2 <= 0.0));
    float distance_to_edge = min(segment_distance(p, a, b), min(segment_distance(p, b, c), segment_distance(p, c, a)));
    return inside ? 1.0 - smoothstep(thickness, thickness + 0.02, distance_to_edge) : 0.0;
}

float icon_plus(float2 uv) {
    float2 p = (uv - 0.5) * 2.0;
    float vertical = sdBox(p, float2(0.12, 0.42));
    float horizontal = sdBox(p, float2(0.42, 0.12));
    float distance_field = min(vertical, horizontal);
    return 1.0 - smoothstep(0.02, 0.08, distance_field);
}

float icon_stop(float2 uv) {
    float2 p = (uv - 0.5) * 2.0;
    float distance_field = sdBox(p, float2(0.34, 0.34));
    return 1.0 - smoothstep(0.02, 0.08, distance_field);
}

float icon_play(float2 uv) {
    float2 p = uv;
    return triangle_mask(p, float2(0.30, 0.22), float2(0.74, 0.50), float2(0.30, 0.78), 0.015);
}

float atlas_alpha(int x, int y) {
    x = clamp(x, 0, FONT_ATLAS_WIDTH - 1);
    y = max(y, 0);
    uint value = GlyphRows[(y * FONT_ATLAS_WIDTH) + x];
    return ((float)value) / 255.0;
}

float text_mask(float glyph, float2 uv) {
    int glyph_index = clamp((int)glyph, 0, 127);
    int atlas_column = glyph_index % FONT_ATLAS_COLUMNS;
    int atlas_row = glyph_index / FONT_ATLAS_COLUMNS;
    float sample_x = (atlas_column * FONT_ATLAS_CELL_WIDTH) + (saturate(uv.x) * (FONT_ATLAS_CELL_WIDTH - 1));
    float sample_y = (atlas_row * FONT_ATLAS_CELL_HEIGHT) + (saturate(uv.y) * (FONT_ATLAS_CELL_HEIGHT - 1));
    int x0 = (int)floor(sample_x);
    int y0 = (int)floor(sample_y);
    int x1 = min(x0 + 1, FONT_ATLAS_WIDTH - 1);
    int y1 = min(y0 + 1, (FONT_ATLAS_CELL_HEIGHT * 8) - 1);
    float tx = frac(sample_x);
    float ty = frac(sample_y);

    float a00 = atlas_alpha(x0, y0);
    float a10 = atlas_alpha(x1, y0);
    float a01 = atlas_alpha(x0, y1);
    float a11 = atlas_alpha(x1, y1);
    float top = lerp(a00, a10, tx);
    float bottom = lerp(a01, a11, tx);
    return lerp(top, bottom, ty);
}

float border_mask(float2 uv) {
    float edge = min(min(uv.x, 1.0 - uv.x), min(uv.y, 1.0 - uv.y));
    return smoothstep(0.0, 0.03, edge);
}

float4 apply_blue_background(float2 uv, float4 color) {
    float waves = 0.5 + 0.5 * sin((uv.x * 11.0) + (uv.y * 17.0));
    float horizon = smoothstep(0.15, 0.95, uv.y);
    float glow = lerp(0.82, 1.18, waves * horizon);
    return float4(color.rgb * glow, color.a);
}

float4 apply_sidecar(float2 uv, float4 color) {
    float bands = 0.92 + 0.08 * sin(uv.y * 38.0);
    return float4(color.rgb * bands, color.a);
}

float4 apply_drag(float2 uv, float4 color) {
    float stripe = smoothstep(0.48, 0.52, abs(uv.y - 0.5));
    float sheen = 0.9 + (0.08 * sin(uv.x * 20.0));
    return float4(color.rgb * (sheen + (0.05 * stripe)), color.a);
}

float4 apply_code(float2 uv, float4 color) {
    float scan = 0.93 + 0.04 * sin(uv.y * 120.0);
    float vignette = 1.0 - (0.08 * distance(uv, float2(0.5, 0.5)));
    return float4(color.rgb * scan * vignette, color.a);
}

float4 apply_result(float2 uv, float4 color) {
    float warmth = 0.88 + 0.12 * sin((uv.x + uv.y) * 20.0);
    return float4(color.rgb * warmth, color.a);
}

float4 apply_button(float2 uv, float4 color, float effect) {
    float highlight = 1.0 - (0.18 * distance(uv, float2(0.4, 0.35)));
    float4 shaded = float4(color.rgb * highlight, color.a);
    float mask = 0.0;
    if (effect < 5.5) {
        mask = icon_play(uv);
    } else if (effect < 6.5) {
        mask = icon_stop(uv);
    } else {
        mask = icon_plus(uv);
    }
    shaded.rgb = lerp(shaded.rgb, float3(0.94, 0.95, 0.98), mask);
    return shaded;
}

float4 PSMain(PsInput input) : SV_TARGET {
    if (input.effect > 7.5) {
        float coverage = text_mask(input.glyph, input.uv);
        return float4(input.color.rgb, input.color.a * coverage);
    }

    float4 shaded = input.color;
    if (input.effect < 0.5) {
        shaded = apply_blue_background(input.uv, input.color);
    } else if (input.effect < 1.5) {
        shaded = apply_sidecar(input.uv, input.color);
    } else if (input.effect < 2.5) {
        shaded = apply_drag(input.uv, input.color);
    } else if (input.effect < 3.5) {
        shaded = apply_code(input.uv, input.color);
    } else if (input.effect < 4.5) {
        shaded = apply_result(input.uv, input.color);
    } else {
        shaded = apply_button(input.uv, input.color, input.effect);
    }

    float mask = border_mask(input.uv);
    float3 border = lerp(float3(0.95, 0.95, 0.98), shaded.rgb, mask);
    return float4(border, shaded.a);
}