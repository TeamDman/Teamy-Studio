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

cbuffer ParamStruct : register(b0)
{
    float4 slug_matrix[4];
    float4 slug_viewport;
};

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

    if (input.effect > 7.5 && any(input.normal != 0.0.xx)) {
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

        if (max(max(p12.x, p12.z), p3.x) * pixelsPerEm.x < -0.5) {
            break;
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

        if (max(max(p12.y, p12.w), p3.y) * pixelsPerEm.y < -0.5) {
            break;
        }

        uint vcode = CalcRootCode(p12.x, p12.z, p3.x);
        if (vcode != 0U) {
            float2 vr = SolveVertPoly(p12, p3) * pixelsPerEm.y;
            if ((vcode & 1U) != 0U) {
                ycov -= saturate(vr.x + 0.5);
                ywgt = max(ywgt, saturate(1.0 - abs(vr.x) * 2.0));
            }
            if (vcode > 1U) {
                ycov += saturate(vr.y + 0.5);
                ywgt = max(ywgt, saturate(1.0 - abs(vr.y) * 2.0));
            }
        }
    }

    return CalcCoverage(xcov, ycov, xwgt, ywgt);
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
        float coverage = slug_coverage(input.uv, input.glyph, input.glyphData, input.banding);
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