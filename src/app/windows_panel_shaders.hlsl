struct VsInput {
    float3 position : POSITION;
    float4 color : COLOR;
    float2 uv : TEXCOORD;
    float effect : EFFECT;
    float glyph : GLYPH;
    float4 glyphData : GLYPHDATA;
    float4 banding : BANDING;
    float2 normal : NORMAL;
    float4 jacobian : JACOBIAN;
    float2 padding : VIEWPORT;
};

struct PsInput {
    float4 position : SV_POSITION;
    float4 color : COLOR;
    float2 uv : TEXCOORD;
    float effect : EFFECT;
    float glyph : GLYPH;
    float4 glyphData : GLYPHDATA;
    float4 banding : BANDING;
};

Buffer<float4> CurveData : register(t0);
Buffer<uint> BandData : register(t1);
Buffer<uint> SpriteAtlasData : register(t2);

cbuffer ParamStruct : register(b0)
{
    float4 slug_matrix[4];
    float4 slug_viewport;
    float4 scene_time;
    float4 sprite_atlas;
};

float PanelTime() {
    return scene_time.x;
}

#include "windows_chrome_shaders.hlsl"

float4 premultiply_alpha(float4 color) {
    return float4(color.rgb * color.a, color.a);
}

float2 SlugDilate(float2 position, float2 texcoord, float2 normal, float4 jacobian, out float2 sampleCoord) {
    float2 n = normalize(normal);
    float s = dot(slug_matrix[3].xy, position) + slug_matrix[3].w;
    float t = dot(slug_matrix[3].xy, n);

    float u = (s * dot(slug_matrix[0].xy, n) - t * (dot(slug_matrix[0].xy, position) + slug_matrix[0].w)) * slug_viewport.x;
    float v = (s * dot(slug_matrix[1].xy, n) - t * (dot(slug_matrix[1].xy, position) + slug_matrix[1].w)) * slug_viewport.y;

    float s2 = s * s;
    float st = s * t;
    float uv = max(u * u + v * v, 1.0 / 16777216.0);
    float denom = uv - st * st;
    float2 d = n * (s2 * (st + sqrt(uv)) / max(abs(denom), 1.0 / 16777216.0));

    sampleCoord = texcoord + float2(dot(d, jacobian.xy), dot(d, jacobian.zw));
    return position + d;
}

PsInput VSMain(VsInput input) {
    PsInput output;
    float2 position = input.position.xy;
    float2 uv = input.uv;

    if (input.effect > 9.5 && any(input.normal != 0.0.xx)) {
        position = SlugDilate(position, uv, input.normal, input.jacobian, uv);
    }

    output.position.x = position.x * slug_matrix[0].x + position.y * slug_matrix[0].y + slug_matrix[0].w;
    output.position.y = position.x * slug_matrix[1].x + position.y * slug_matrix[1].y + slug_matrix[1].w;
    output.position.z = position.x * slug_matrix[2].x + position.y * slug_matrix[2].y + slug_matrix[2].w;
    output.position.w = position.x * slug_matrix[3].x + position.y * slug_matrix[3].y + slug_matrix[3].w;
    output.color = input.color;
    output.uv = uv;
    output.effect = input.effect;
    output.glyph = input.glyph;
    output.glyphData = input.glyphData;
    output.banding = input.banding;
    return output;
}

float4 unpack_rgba8(uint packed) {
    float r = (packed & 0xFFU) / 255.0;
    float g = ((packed >> 8U) & 0xFFU) / 255.0;
    float b = ((packed >> 16U) & 0xFFU) / 255.0;
    float a = ((packed >> 24U) & 0xFFU) / 255.0;
    return float4(r, g, b, a);
}

float4 sample_sprite_atlas(float2 uv) {
    uint atlas_width = max((uint)sprite_atlas.x, 1U);
    uint atlas_height = max((uint)sprite_atlas.y, 1U);
    uint x = min((uint)round(saturate(uv.x) * (atlas_width - 1U)), atlas_width - 1U);
    uint y = min((uint)round(saturate(uv.y) * (atlas_height - 1U)), atlas_height - 1U);
    uint index = y * atlas_width + x;
    return unpack_rgba8(SpriteAtlasData[index]);
}

uint CalcRootCode(float y1, float y2, float y3) {
    uint i1 = asuint(y1) >> 31U;
    uint i2 = asuint(y2) >> 30U;
    uint i3 = asuint(y3) >> 29U;

    uint shift = (i2 & 2U) | (i1 & ~2U);
    shift = (i3 & 4U) | (shift & ~4U);
    return ((0x2E74U >> shift) & 0x0101U);
}

float2 SolveHorizPoly(float4 p12, float2 p3) {
    float2 a = p12.xy - p12.zw * 2.0 + p3;
    float2 b = p12.xy - p12.zw;
    float ra = 1.0 / a.y;
    float rb = 0.5 / b.y;
    float d = sqrt(max(b.y * b.y - a.y * p12.y, 0.0));
    float t1 = (b.y - d) * ra;
    float t2 = (b.y + d) * ra;
    if (abs(a.y) < 1.0 / 65536.0) t1 = t2 = p12.y * rb;
    return float2((a.x * t1 - b.x * 2.0) * t1 + p12.x, (a.x * t2 - b.x * 2.0) * t2 + p12.x);
}

float2 SolveVertPoly(float4 p12, float2 p3) {
    float2 a = p12.xy - p12.zw * 2.0 + p3;
    float2 b = p12.xy - p12.zw;
    float ra = 1.0 / a.x;
    float rb = 0.5 / b.x;
    float d = sqrt(max(b.x * b.x - a.x * p12.x, 0.0));
    float t1 = (b.x - d) * ra;
    float t2 = (b.x + d) * ra;
    if (abs(a.x) < 1.0 / 65536.0) t1 = t2 = p12.x * rb;
    return float2((a.y * t1 - b.y * 2.0) * t1 + p12.y, (a.y * t2 - b.y * 2.0) * t2 + p12.y);
}

float CalcCoverage(float xcov, float ycov, float xwgt, float ywgt) {
    return saturate(max(abs(xcov * xwgt + ycov * ywgt) / max(xwgt + ywgt, 1.0 / 65536.0), min(abs(xcov), abs(ycov))));
}

static const float SLUG_HORIZONTAL_COVERAGE_EPSILON = 1.0 / 65536.0;

bool IsDegenerateQuadratic(float4 p12, float2 p3) {
    float2 a = p12.xy - p12.zw * 2.0 + p3;
    return all(abs(a) <= float2(1.0 / 1024.0, 1.0 / 1024.0));
}

bool ShouldUseDegenerateLineFallback(float4 p12, float2 p3) {
    return IsDegenerateQuadratic(p12, p3);
}

bool CrossesZeroHalfOpen(float a, float b) {
    return ((a <= 0.0) && (b > 0.0)) || ((b <= 0.0) && (a > 0.0));
}

void ApplyDegenerateHorizontalCoverage(
    float2 p0,
    float2 p1,
    float pixelsPerEm,
    inout float xcov,
    inout float xwgt
) {
    p0.y += SLUG_HORIZONTAL_COVERAGE_EPSILON;
    p1.y += SLUG_HORIZONTAL_COVERAGE_EPSILON;
    float dy = p1.y - p0.y;
    if (CrossesZeroHalfOpen(p0.y, p1.y) && abs(dy) > (1.0 / 65536.0)) {
        float t = -p0.y / dy;
        float xr = (p0.x + (p1.x - p0.x) * t) * pixelsPerEm;
        float sample = saturate(xr + 0.5);
        xcov += (p1.y > p0.y) ? sample : -sample;
        xwgt = max(xwgt, saturate(1.0 - abs(xr) * 2.0));
    }
}

void ApplyDegenerateVerticalCoverage(
    float2 p0,
    float2 p1,
    float pixelsPerEm,
    inout float ycov,
    inout float ywgt
) {
    float dx = p1.x - p0.x;
    if (CrossesZeroHalfOpen(p0.x, p1.x) && abs(dx) > (1.0 / 65536.0)) {
        float t = -p0.x / dx;
        float yr = (p0.y + (p1.y - p0.y) * t) * pixelsPerEm;
        float sample = saturate(-yr + 0.5);
        ycov += (p1.x > p0.x) ? sample : -sample;
        ywgt = max(ywgt, saturate(1.0 - abs(yr) * 2.0));
    }
}

uint ClampBandIndex(float coord, float scale, float offset, uint bandMax) {
    return (uint)clamp((int)(coord * scale + offset), 0, (int)bandMax);
}

uint2 LoadBandEntry(uint bandStart, uint bandIndex) {
    uint entry = bandStart + (bandIndex * 2U);
    return uint2(BandData[entry], BandData[entry + 1U]);
}

float slug_coverage(float2 renderCoord, float bandStartFloat, float4 glyphData, float4 banding) {
    int curveStart = (int)glyphData.x;
    int curveCount = (int)glyphData.y;
    if (curveCount <= 0) {
        return 0.0;
    }

    uint bandStart = (uint)bandStartFloat;
    uint bandMaxX = (uint)glyphData.z;
    uint bandMaxY = (uint)glyphData.w;
    float2 pixelsPerEm = 1.0 / fwidth(renderCoord);
    float xcov = 0.0;
    float ycov = 0.0;
    float xwgt = 0.0;
    float ywgt = 0.0;

    uint horizontalBand = ClampBandIndex(renderCoord.y, banding.y, banding.w, bandMaxY);
    uint2 horizontalEntry = LoadBandEntry(bandStart, horizontalBand);

    [loop]
    for (uint offset = 0U; offset < horizontalEntry.x; offset++) {
        int curveIndex = (int)BandData[horizontalEntry.y + offset];
        int baseIndex = curveStart + (curveIndex * 2);
        float4 p12 = CurveData[baseIndex] - float4(renderCoord, renderCoord);
        float2 p3 = CurveData[baseIndex + 1].xy - renderCoord;
        p12.y += SLUG_HORIZONTAL_COVERAGE_EPSILON;
        p12.w += SLUG_HORIZONTAL_COVERAGE_EPSILON;
        p3.y += SLUG_HORIZONTAL_COVERAGE_EPSILON;

        if (max(max(p12.x, p12.z), p3.x) * pixelsPerEm.x < -0.5) {
            break;
        }

        if (ShouldUseDegenerateLineFallback(p12, p3)) {
            ApplyDegenerateHorizontalCoverage(p12.xy, p3, pixelsPerEm.x, xcov, xwgt);
            continue;
        }

        uint hcode = CalcRootCode(p12.y, p12.w, p3.y);
        if (hcode != 0U) {
            float2 hr = SolveHorizPoly(p12, p3) * pixelsPerEm.x;
            if ((hcode & 1U) != 0U) {
                xcov += saturate(hr.x + 0.5);
                xwgt = max(xwgt, saturate(1.0 - abs(hr.x) * 2.0));
            }
            if (hcode > 1U) {
                xcov -= saturate(hr.y + 0.5);
                xwgt = max(xwgt, saturate(1.0 - abs(hr.y) * 2.0));
            }
        }
    }

    uint verticalBandStart = bandStart + ((bandMaxY + 1U) * 2U);
    uint verticalBand = ClampBandIndex(renderCoord.x, banding.x, banding.z, bandMaxX);
    uint2 verticalEntry = LoadBandEntry(verticalBandStart, verticalBand);

    [loop]
    for (uint offset = 0U; offset < verticalEntry.x; offset++) {
        int curveIndex = (int)BandData[verticalEntry.y + offset];
        int baseIndex = curveStart + (curveIndex * 2);
        float4 p12 = CurveData[baseIndex] - float4(renderCoord, renderCoord);
        float2 p3 = CurveData[baseIndex + 1].xy - renderCoord;

        if (min(min(p12.y, p12.w), p3.y) * pixelsPerEm.y > 0.5) {
            break;
        }

        if (ShouldUseDegenerateLineFallback(p12, p3)) {
            ApplyDegenerateVerticalCoverage(p12.xy, p3, pixelsPerEm.y, ycov, ywgt);
            continue;
        }

        uint vcode = CalcRootCode(p12.x, p12.z, p3.x);
        if (vcode != 0U) {
            float2 vr = SolveVertPoly(p12, p3) * pixelsPerEm.y;
            if ((vcode & 1U) != 0U) {
                ycov -= saturate(-vr.x + 0.5);
                ywgt = max(ywgt, saturate(1.0 - abs(vr.x) * 2.0));
            }
            if (vcode > 1U) {
                ycov += saturate(-vr.y + 0.5);
                ywgt = max(ywgt, saturate(1.0 - abs(vr.y) * 2.0));
            }
        }
    }

    return CalcCoverage(xcov, ycov, xwgt, ywgt);
}

float4 apply_blue_background(float2 uv, float4 color) {
    float t = PanelTime();
    float drift = sin((uv.x * 6.5) + (uv.y * 4.2) - (t * 0.45));
    float ripple = sin((uv.x * 18.0) - (uv.y * 7.0) + (t * 0.8));
    float horizon = smoothstep(0.08, 0.96, uv.y);
    float glow = 0.9 + (0.12 * drift * horizon) + (0.05 * ripple);
    return float4(color.rgb * glow, 0.5);
}

float4 apply_drag(float2 uv, float4 color) {
    float t = PanelTime();
    float stripe = smoothstep(0.48, 0.52, abs(uv.y - 0.5));
    float sweep = sin((uv.x * 15.0) - (t * 1.4));
    float sheen = 0.92 + (0.06 * sweep) + (0.04 * sin((uv.y * 9.0) + (t * 0.8)));
    return float4(color.rgb * (sheen + (0.05 * stripe)), color.a);
}

float4 apply_code(float2 uv, float4 color) {
    float t = PanelTime();
    float scan = 0.95 + (0.02 * sin((uv.y * 110.0) - (t * 2.2)));
    float drift = 0.98 + (0.03 * sin((uv.x * 4.0) + (uv.y * 6.5) + (t * 0.35)));
    return float4(color.rgb * scan * drift, color.a);
}

float4 apply_result(float2 uv, float4 color) {
    float t = PanelTime();
    float warmth = 0.9 + (0.08 * sin(((uv.x + uv.y) * 16.0) - (t * 0.6)));
    float wave = 0.97 + (0.04 * sin((uv.x * 12.0) + (t * 0.9)));
    warmth *= wave;
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
        mask = icon_diagnostics(uv);
    }
    shaded.rgb = lerp(shaded.rgb, float3(0.94, 0.95, 0.98), mask);
    return shaded;
}

float4 apply_scene_button_card(float2 uv, float4 color, float4 state) {
    float t = PanelTime();
    float near = state.x;
    float hover = state.y;
    float pressed = state.z;
    float click = state.w;
    float center = 1.0 - smoothstep(0.0, 0.78, distance(uv, float2(0.5, 0.44)));
    float rim = 1.0 - smoothstep(0.18, 0.5, abs(uv.y - 0.08));
    float sweep = 0.5 + (0.5 * sin((uv.x * 14.0) - (t * (1.2 + hover))));
    float shimmer = 0.5 + (0.5 * sin((uv.y * 22.0) + (t * 1.8)));
    float pulse = click * (0.5 + (0.5 * sin(((uv.x + uv.y) * 18.0) - (t * 4.2))));
    float intensity = 0.88 + (near * 0.06) + (hover * 0.10) + (center * (0.08 + (0.07 * hover))) + (sweep * 0.05) + (shimmer * 0.03) + (pulse * 0.16) - (pressed * 0.10);
    float3 tint = color.rgb * lerp(float3(0.86, 0.90, 0.96), float3(1.04, 1.05, 1.08), hover + (click * 0.35));
    float top_glow = rim * (0.08 + (0.12 * hover) + (0.10 * click));
    return float4(tint * (intensity + top_glow), color.a);
}

float4 apply_scene_body(float2 uv, float4 color) {
    float t = PanelTime();
    float wash = 0.92 + (0.05 * sin((uv.x * 4.0) + (t * 0.55))) + (0.04 * sin((uv.y * 7.0) - (t * 0.42)));
    float grain = 0.98 + (0.03 * sin((uv.x * 36.0) + (uv.y * 20.0) + (t * 0.9)));
    return float4(color.rgb * wash * grain, color.a);
}

float4 apply_terminal_scrollbar_track(float2 uv, float4 color) {
    float t = PanelTime();
    float hover = saturate((color.a - 0.78) / 0.12);
    float center = 1.0 - smoothstep(0.08, 0.95, abs((uv.x - 0.5) * 2.0));
    float ribbon = 0.5 + (0.5 * sin((uv.y * 24.0) - (t * 2.1)));
    float shimmer = 0.5 + (0.5 * sin((uv.x * 8.0) + (uv.y * 10.0) + (t * 1.3)));
    float pulse = 0.94 + (0.06 * sin((uv.y * 7.0) + (t * 0.9)));
    float glow = pulse + (center * (0.12 + (0.10 * hover))) + (ribbon * 0.08) + (shimmer * 0.04);
    float3 tint = lerp(color.rgb * float3(0.92, 0.86, 1.08), color.rgb * float3(1.08, 0.94, 1.20), hover);
    return float4(tint * glow, color.a);
}

float4 apply_terminal_scrollbar_thumb(float2 uv, float4 color) {
    float t = PanelTime();
    float hover = saturate((color.a - 0.88) / 0.08);
    float grabbed = saturate((color.a - 0.97) / 0.03);
    float center = 1.0 - smoothstep(0.10, 0.98, abs((uv.x - 0.5) * 2.0));
    float ribbon = 0.5 + (0.5 * sin((uv.y * 32.0) - (t * (2.6 + grabbed))));
    float sparkle = 0.5 + (0.5 * sin((uv.x * 9.0) + (uv.y * 18.0) + (t * 3.2)));
    float cap = 0.94 + (0.06 * sin((uv.x * 15.0) - (t * 1.1)));
    float intensity = cap + (center * (0.16 + (0.10 * hover) + (0.08 * grabbed))) + (ribbon * (0.08 + (0.08 * grabbed))) + (sparkle * (0.03 + (0.06 * hover)));
    float3 tint = lerp(color.rgb * float3(1.02, 0.92, 1.04), float3(1.00, 0.84, 1.00), grabbed);
    return float4(tint * intensity, color.a);
}

float4 PSMain(PsInput input) : SV_TARGET {
    if (input.effect > 11.5 && input.effect < 12.5) {
        float coverage = slug_coverage(input.uv, input.glyph, input.glyphData, input.banding);
        return premultiply_alpha(float4(input.color.rgb, input.color.a * coverage));
    }

    if (input.effect > 12.5 && input.effect < 13.5) {
        float4 sprite = sample_sprite_atlas(input.uv);
        return premultiply_alpha(float4(sprite.rgb * input.color.rgb, sprite.a * input.color.a));
    }

    if (input.effect > 7.5 && input.effect < 9.5) {
        return premultiply_alpha(input.color);
    }

    float4 shaded = input.color;
    if (input.effect > 15.5) {
        shaded = apply_window_chrome_button(input.uv, input.color, input.glyphData, input.effect);
    } else if (input.effect > 14.5) {
        shaded = apply_scene_body(input.uv, input.color);
    } else if (input.effect > 13.5) {
        shaded = apply_scene_button_card(input.uv, input.color, input.glyphData);
    } else if (input.effect > 10.5) {
        shaded = apply_terminal_scrollbar_thumb(input.uv, input.color);
    } else if (input.effect > 9.5) {
        shaded = apply_terminal_scrollbar_track(input.uv, input.color);
    } else if (input.effect < 0.5) {
        shaded = apply_blue_background(input.uv, input.color);
    } else if (input.effect < 1.5) {
        shaded = apply_garden_frame(input.uv, input.color, input.glyphData);
    } else if (input.effect < 2.5) {
        shaded = apply_drag(input.uv, input.color);
    } else if (input.effect < 3.5) {
        shaded = apply_code(input.uv, input.color);
    } else if (input.effect < 4.5) {
        shaded = apply_result(input.uv, input.color);
    } else {
        shaded = apply_button(input.uv, input.color, input.effect);
    }

    return premultiply_alpha(shaded);
}