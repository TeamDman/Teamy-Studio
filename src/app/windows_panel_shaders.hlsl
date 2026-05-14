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
    float4 localBounds : LOCALBOUNDS;
    float2 debugData : VIEWPORT;
};

struct PsInput {
    float4 position : SV_POSITION;
    float4 color : COLOR;
    float2 uv : TEXCOORD;
    float effect : EFFECT;
    float glyph : GLYPH;
    float4 glyphData : GLYPHDATA;
    float4 banding : BANDING;
    float4 uvBounds : JACOBIAN;
    float4 localBounds : LOCALBOUNDS;
    float debugId : VIEWPORT;
    float transformedFlag : VIEWPORT1;
};

Buffer<float4> CurveData : register(t0);
Buffer<uint> BandData : register(t1);
Buffer<uint> SpriteAtlasData : register(t2);

cbuffer ParamStruct : register(b0)
{
    float4 slug_matrix[4];
    float4 slug_viewport;
    float4 scene_time;
    float4 transformed_text_clip_rect;
    float4 transformed_text_debug_hover;
    float4 transformed_text_projection[2];
    float4 transformed_text_inverse_homography[2];
    float4 sprite_atlas;
};

float PanelTime() {
    return scene_time.x;
}

#include "windows_chrome_shaders.hlsl"

float4 premultiply_alpha(float4 color) {
    return float4(color.rgb * color.a, color.a);
}

float3 rotate_transformed_text_point(float2 localPoint) {
    float4 rotation = transformed_text_projection[1];
    float yawSin = rotation.x;
    float yawCos = rotation.y;
    float pitchSin = rotation.z;
    float pitchCos = rotation.w;

    float3 yawRotated = float3(localPoint.x * yawCos, localPoint.y, -localPoint.x * yawSin);
    return float3(
        yawRotated.x,
        (yawRotated.y * pitchCos) - (yawRotated.z * pitchSin),
        (yawRotated.y * pitchSin) + (yawRotated.z * pitchCos)
    );
}

float4 project_transformed_text_point(float2 localPoint) {
    float4 projection = transformed_text_projection[0];
    float2 center = projection.xy;
    float cameraDistance = projection.z;
    float3 rotated = rotate_transformed_text_point(localPoint);
    float clipW = 1.0 - (rotated.z / cameraDistance);
    float2 centerClip = float2(
        dot(slug_matrix[0].xy, center) + slug_matrix[0].w,
        dot(slug_matrix[1].xy, center) + slug_matrix[1].w
    );
    float2 projectedOffset = float2(
        dot(slug_matrix[0].xy, rotated.xy),
        dot(slug_matrix[1].xy, rotated.xy)
    );
    return float4(
        (centerClip.x * clipW) + projectedOffset.x,
        (centerClip.y * clipW) + projectedOffset.y,
        clipW * 0.5,
        clipW
    );
}

float4 transformed_clip_from_screen(float2 screenPoint, float clipW) {
    float2 ndc = float2(
        (screenPoint.x / max(slug_viewport.x, 1.0)) * 2.0 - 1.0,
        1.0 - (screenPoint.y / max(slug_viewport.y, 1.0)) * 2.0
    );
    return float4(ndc * clipW, clipW * 0.5, clipW);
}

float2 reconstruct_transformed_local_point(float2 screenPoint) {
    float4 row0 = transformed_text_inverse_homography[0];
    float4 row1 = transformed_text_inverse_homography[1];
    float denominator = (row0.w * screenPoint.x) + (row1.w * screenPoint.y) + 1.0;
    return float2(
        ((row0.x * screenPoint.x) + (row0.y * screenPoint.y) + row0.z) / denominator,
        ((row1.x * screenPoint.x) + (row1.y * screenPoint.y) + row1.z) / denominator
    );
}

float remap_range(float value, float sourceMin, float sourceMax, float targetMin, float targetMax) {
    float sourceSpan = max(sourceMax - sourceMin, 1.0 / 65536.0);
    float t = (value - sourceMin) / sourceSpan;
    return targetMin + ((targetMax - targetMin) * t);
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

    if (input.effect > 9.5 && input.debugData.y <= 0.5 && any(input.normal != 0.0.xx)) {
        position = SlugDilate(position, uv, input.normal, input.jacobian, uv);
    }

    if (input.debugData.y > 0.5) {
        output.position = transformed_clip_from_screen(position, max(input.position.z, 1.0 / 65536.0));
    } else {
        output.position.x = position.x * slug_matrix[0].x + position.y * slug_matrix[0].y + slug_matrix[0].w;
        output.position.y = position.x * slug_matrix[1].x + position.y * slug_matrix[1].y + slug_matrix[1].w;
        output.position.z = position.x * slug_matrix[2].x + position.y * slug_matrix[2].y + slug_matrix[2].w;
        output.position.w = position.x * slug_matrix[3].x + position.y * slug_matrix[3].y + slug_matrix[3].w;
    }
    output.color = input.color;
    output.uv = uv;
    output.effect = input.effect;
    output.glyph = input.glyph;
    output.glyphData = input.glyphData;
    output.banding = input.banding;
    output.uvBounds = input.jacobian;
    output.localBounds = input.localBounds;
    output.debugId = input.debugData.x;
    output.transformedFlag = input.debugData.y;
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
    bool leftRay,
    inout float xcov,
    inout float xwgt
) {
    p0.y += SLUG_HORIZONTAL_COVERAGE_EPSILON;
    p1.y += SLUG_HORIZONTAL_COVERAGE_EPSILON;
    float dy = p1.y - p0.y;
    if (CrossesZeroHalfOpen(p0.y, p1.y) && abs(dy) > (1.0 / 65536.0)) {
        float t = -p0.y / dy;
        float xr = (p0.x + (p1.x - p0.x) * t) * pixelsPerEm;
        float sample = leftRay ? saturate(0.5 - xr) : saturate(xr + 0.5);
        xcov += (p1.y > p0.y) ? sample : -sample;
        xwgt = max(xwgt, saturate(1.0 - abs(xr) * 2.0));
    }
}

void ApplyDegenerateVerticalCoverage(
    float2 p0,
    float2 p1,
    float pixelsPerEm,
    bool leftRay,
    inout float ycov,
    inout float ywgt
) {
    float dx = p1.x - p0.x;
    if (CrossesZeroHalfOpen(p0.x, p1.x) && abs(dx) > (1.0 / 65536.0)) {
        float t = -p0.x / dx;
        float yr = (p0.y + (p1.y - p0.y) * t) * pixelsPerEm;
        float sample = leftRay ? saturate(0.5 - yr) : saturate(yr + 0.5);
        ycov += (p1.x > p0.x) ? sample : -sample;
        ywgt = max(ywgt, saturate(1.0 - abs(yr) * 2.0));
    }
}

uint ClampBandIndex(float coord, float scale, float offset, uint bandMax) {
    return (uint)clamp((int)(coord * scale + offset), 0, (int)bandMax);
}

uint4 LoadBandEntry(uint bandStart, uint bandIndex) {
    uint entry = bandStart + (bandIndex * 4U);
    return uint4(BandData[entry], BandData[entry + 1U], BandData[entry + 2U], BandData[entry + 3U]);
}

float slug_coverage_single_sample(float2 renderCoord, float bandStartFloat, float4 glyphData, float4 banding) {
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
    uint4 horizontalEntry = LoadBandEntry(bandStart, horizontalBand);
    bool horizontalLeftRay = renderCoord.x < asfloat(horizontalEntry.w);
    uint horizontalCurveStart = horizontalLeftRay ? horizontalEntry.z : horizontalEntry.y;

    [loop]
    for (uint offset = 0U; offset < horizontalEntry.x; offset++) {
        int curveIndex = (int)BandData[horizontalCurveStart + offset];
        int baseIndex = curveStart + (curveIndex * 2);
        float4 p12 = CurveData[baseIndex] - float4(renderCoord, renderCoord);
        float2 p3 = CurveData[baseIndex + 1].xy - renderCoord;
        p12.y += SLUG_HORIZONTAL_COVERAGE_EPSILON;
        p12.w += SLUG_HORIZONTAL_COVERAGE_EPSILON;
        p3.y += SLUG_HORIZONTAL_COVERAGE_EPSILON;

        if (horizontalLeftRay) {
            if (min(min(p12.x, p12.z), p3.x) * pixelsPerEm.x > 0.5) {
                break;
            }
        } else {
            if (max(max(p12.x, p12.z), p3.x) * pixelsPerEm.x < -0.5) {
                break;
            }
        }

        if (ShouldUseDegenerateLineFallback(p12, p3)) {
            ApplyDegenerateHorizontalCoverage(p12.xy, p3, pixelsPerEm.x, horizontalLeftRay, xcov, xwgt);
            continue;
        }

        uint hcode = CalcRootCode(p12.y, p12.w, p3.y);
        if (hcode != 0U) {
            float2 hr = SolveHorizPoly(p12, p3) * pixelsPerEm.x;
            float2 hcov = horizontalLeftRay
                ? clamp(0.5.xx - hr, 0.0.xx, 1.0.xx)
                : clamp(hr + 0.5.xx, 0.0.xx, 1.0.xx);
            if ((hcode & 1U) != 0U) {
                xcov += hcov.x;
                xwgt = max(xwgt, saturate(1.0 - abs(hr.x) * 2.0));
            }
            if (hcode > 1U) {
                xcov -= hcov.y;
                xwgt = max(xwgt, saturate(1.0 - abs(hr.y) * 2.0));
            }
        }
    }

    uint verticalBandStart = bandStart + ((bandMaxY + 1U) * 4U);
    uint verticalBand = ClampBandIndex(renderCoord.x, banding.x, banding.z, bandMaxX);
    uint4 verticalEntry = LoadBandEntry(verticalBandStart, verticalBand);
    bool verticalLeftRay = renderCoord.y < asfloat(verticalEntry.w);
    uint verticalCurveStart = verticalLeftRay ? verticalEntry.z : verticalEntry.y;

    [loop]
    for (uint verticalOffset = 0U; verticalOffset < verticalEntry.x; verticalOffset++) {
        int curveIndex = (int)BandData[verticalCurveStart + verticalOffset];
        int baseIndex = curveStart + (curveIndex * 2);
        float4 p12 = CurveData[baseIndex] - float4(renderCoord, renderCoord);
        float2 p3 = CurveData[baseIndex + 1].xy - renderCoord;

        if (verticalLeftRay) {
            if (min(min(p12.y, p12.w), p3.y) * pixelsPerEm.y > 0.5) {
                break;
            }
        } else {
            if (max(max(p12.y, p12.w), p3.y) * pixelsPerEm.y < -0.5) {
                break;
            }
        }

        if (ShouldUseDegenerateLineFallback(p12, p3)) {
            ApplyDegenerateVerticalCoverage(p12.xy, p3, pixelsPerEm.y, verticalLeftRay, ycov, ywgt);
            continue;
        }

        uint vcode = CalcRootCode(p12.x, p12.z, p3.x);
        if (vcode != 0U) {
            float2 vr = SolveVertPoly(p12, p3) * pixelsPerEm.y;
            float2 vcov = verticalLeftRay
                ? clamp(0.5.xx - vr, 0.0.xx, 1.0.xx)
                : clamp(vr + 0.5.xx, 0.0.xx, 1.0.xx);
            if ((vcode & 1U) != 0U) {
                ycov -= vcov.x;
                ywgt = max(ywgt, saturate(1.0 - abs(vr.x) * 2.0));
            }
            if (vcode > 1U) {
                ycov += vcov.y;
                ywgt = max(ywgt, saturate(1.0 - abs(vr.y) * 2.0));
            }
        }
    }

    return CalcCoverage(xcov, ycov, xwgt, ywgt);
}

float slug_coverage(float2 renderCoord, float bandStartFloat, float4 glyphData, float4 banding) {
    float2 emsPerPixel = max(fwidth(renderCoord), float2(1.0 / 65536.0, 1.0 / 65536.0));
    float2 sampleStep = emsPerPixel * 0.25;
    float coverage = 0.0;
    coverage += slug_coverage_single_sample(renderCoord + float2(-sampleStep.x, -sampleStep.y), bandStartFloat, glyphData, banding);
    coverage += slug_coverage_single_sample(renderCoord + float2(sampleStep.x, -sampleStep.y), bandStartFloat, glyphData, banding);
    coverage += slug_coverage_single_sample(renderCoord + float2(-sampleStep.x, sampleStep.y), bandStartFloat, glyphData, banding);
    coverage += slug_coverage_single_sample(renderCoord + float2(sampleStep.x, sampleStep.y), bandStartFloat, glyphData, banding);
    return coverage * 0.25;
}

float2 evaluate_quadratic(float2 p0, float2 p1, float2 p2, float t) {
    float omt = 1.0 - t;
    return (omt * omt * p0) + (2.0 * omt * t * p1) + (t * t * p2);
}

float distance_to_segment(float2 samplePoint, float2 segmentStart, float2 segmentEnd) {
    float2 delta = segmentEnd - segmentStart;
    float length_squared = dot(delta, delta);
    if (length_squared <= (1.0 / 65536.0)) {
        return length(samplePoint - segmentStart);
    }
    float t = saturate(dot(samplePoint - segmentStart, delta) / length_squared);
    float2 projection = segmentStart + (delta * t);
    return length(samplePoint - projection);
}

float3 debug_curve_color(float curveIndex) {
    float tint = 0.92 + (0.08 * frac(curveIndex * 0.37));
    return float3(0.18, 0.96, 0.42) * tint;
}

float curve_distance(float2 samplePoint, int curveStart, int curveIndex) {
    int baseIndex = curveStart + (curveIndex * 2);
    float4 p12 = CurveData[baseIndex];
    float2 p0 = p12.xy;
    float2 p1 = p12.zw;
    float2 p2 = CurveData[baseIndex + 1].xy;

    float bestCurveDistance = 1e9;
    float2 previous = p0;
    [unroll]
    for (int segmentIndex = 1; segmentIndex <= 12; segmentIndex++) {
        float t = segmentIndex / 12.0;
        float2 current = evaluate_quadratic(p0, p1, p2, t);
        bestCurveDistance = min(bestCurveDistance, distance_to_segment(samplePoint, previous, current));
        previous = current;
    }

    return bestCurveDistance;
}

float rect_outline_alpha(float2 samplePoint, float2 minPoint, float2 maxPoint, float pixelsPerEm) {
    float withinX = step(minPoint.x, samplePoint.x) * step(samplePoint.x, maxPoint.x);
    float withinY = step(minPoint.y, samplePoint.y) * step(samplePoint.y, maxPoint.y);
    float edgeDistance = min(
        min(abs(samplePoint.x - minPoint.x), abs(samplePoint.x - maxPoint.x)),
        min(abs(samplePoint.y - minPoint.y), abs(samplePoint.y - maxPoint.y))
    ) * pixelsPerEm;
    return (withinX * withinY) * (1.0 - smoothstep(0.9, 2.8, edgeDistance));
}

float point_marker_alpha(float2 samplePoint, float2 markerPoint, float pixelsPerEm, float radiusPx) {
    float distancePx = length(samplePoint - markerPoint) * pixelsPerEm;
    return 1.0 - smoothstep(radiusPx, radiusPx + 1.2, distancePx);
}

float4 slug_geometry_debug(float2 renderCoord, float4 glyphData, float debugId) {
    int curveStart = (int)glyphData.x;
    int curveCount = (int)glyphData.y;
    if (curveCount <= 0) {
        return float4(0.0, 0.0, 0.0, 0.0);
    }

    float2 renderCoordWidth = max(fwidth(renderCoord), float2(1.0 / 4096.0, 1.0 / 4096.0));
    float pixelsPerEm = 1.0 / min(renderCoordWidth.x, renderCoordWidth.y);
    float bestCurveDistancePx = 1e9;
    float bestHandleDistancePx = 1e9;
    float bestPointDistancePx = 1e9;
    float bestCurveIndex = 0.0;
    float pointAlpha = 0.0;

    [loop]
    for (int curveIndex = 0; curveIndex < curveCount; curveIndex++) {
        int baseIndex = curveStart + (curveIndex * 2);
        float4 p12 = CurveData[baseIndex];
        float2 p0 = p12.xy;
        float2 p1 = p12.zw;
        float2 p2 = CurveData[baseIndex + 1].xy;

        float bestCurveDistance = curve_distance(renderCoord, curveStart, curveIndex);
        float curvePointAlpha = max(
            point_marker_alpha(renderCoord, p0, pixelsPerEm, 1.9),
            max(
                point_marker_alpha(renderCoord, p1, pixelsPerEm, 2.2),
                point_marker_alpha(renderCoord, p2, pixelsPerEm, 1.9)
            )
        );

        float handleDistance = min(
            distance_to_segment(renderCoord, p0, p1),
            distance_to_segment(renderCoord, p1, p2)
        );

        float curveDistancePx = bestCurveDistance * pixelsPerEm;
        if (curveDistancePx < bestCurveDistancePx) {
            bestCurveDistancePx = curveDistancePx;
            bestCurveIndex = curveIndex;
        }
        bestHandleDistancePx = min(bestHandleDistancePx, handleDistance * pixelsPerEm);
        bestPointDistancePx = min(
            bestPointDistancePx,
            min(length(renderCoord - p0), min(length(renderCoord - p1), length(renderCoord - p2))) * pixelsPerEm
        );
        pointAlpha = max(pointAlpha, curvePointAlpha);
    }

    float curveAlpha = 1.0 - smoothstep(1.10, 3.10, bestCurveDistancePx);
    float handleAlpha = 1.0 - smoothstep(0.90, 2.20, bestHandleDistancePx);
    pointAlpha = max(pointAlpha, 1.0 - smoothstep(1.2, 3.0, bestPointDistancePx));
    float3 curveColor = debug_curve_color(bestCurveIndex);
    float3 handleColor = float3(0.58, 1.0, 0.72);
    float3 debugColor = lerp(handleColor, curveColor, saturate(curveAlpha));
    debugColor = lerp(debugColor, float3(1.0, 0.90, 0.42), pointAlpha * 0.92);
    float alpha = max(max(curveAlpha * 0.98, handleAlpha * 0.62), pointAlpha * 0.78);

    if (transformed_text_debug_hover.w > 0.5 && abs(debugId - transformed_text_debug_hover.x) < 0.25) {
        float2 hoverPoint = transformed_text_debug_hover.yz;
        float bestHoverCurveDistance = 1e9;
        int hoveredCurveIndex = 0;
        float2 hoveredMinPoint = float2(0.0, 0.0);
        float2 hoveredMaxPoint = float2(0.0, 0.0);
        [loop]
        for (int curveIndex = 0; curveIndex < curveCount; curveIndex++) {
            int baseIndex = curveStart + (curveIndex * 2);
            float4 p12 = CurveData[baseIndex];
            float2 p0 = p12.xy;
            float2 p1 = p12.zw;
            float2 p2 = CurveData[baseIndex + 1].xy;
            float hoverCurveDistance = curve_distance(hoverPoint, curveStart, curveIndex);
            if (hoverCurveDistance < bestHoverCurveDistance) {
                bestHoverCurveDistance = hoverCurveDistance;
                hoveredCurveIndex = curveIndex;
                hoveredMinPoint = min(min(p0, p1), p2);
                hoveredMaxPoint = max(max(p0, p1), p2);
            }
        }

        float hoveredCurveDistancePx = curve_distance(renderCoord, curveStart, hoveredCurveIndex) * pixelsPerEm;
        float hoveredCurveAlpha = 1.0 - smoothstep(1.2, 4.2, hoveredCurveDistancePx);
        float hoveredBoundsAlpha = rect_outline_alpha(renderCoord, hoveredMinPoint, hoveredMaxPoint, pixelsPerEm);
        float hoveredAlpha = max(hoveredCurveAlpha, hoveredBoundsAlpha * 0.9);
        debugColor = lerp(debugColor, float3(1.0, 0.34, 0.82), hoveredAlpha);
        alpha = max(alpha, hoveredAlpha * 0.96);
    }

    return float4(debugColor, saturate(alpha));
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

float4 apply_target_marker(float2 uv, float4 color, float4 state) {
    float t = PanelTime();
    float isPuck = step(0.5, state.x);
    float hover = saturate(state.y);
    float dragging = saturate(state.z);
    float2 markerCoord = (uv - 0.5) * 2.0;
    float radius = length(markerCoord);
    float edge = 1.0 - smoothstep(0.90, 1.0, radius);
    float ring0 = smoothstep(0.94, 0.82, radius) - smoothstep(0.76, 0.66, radius);
    float ring1 = smoothstep(0.66, 0.56, radius) - smoothstep(0.48, 0.38, radius);
    float center = 1.0 - smoothstep(0.24, 0.34, radius);
    float stripeAngle = atan2(markerCoord.y, markerCoord.x);
    float stripeWave = 0.5 + (0.5 * sin((stripeAngle * 6.0) + (t * (1.4 + dragging))));
    float stripe = smoothstep(0.42, 0.58, stripeWave);
    float3 red = lerp(float3(0.72, 0.12, 0.10), float3(0.94, 0.22, 0.18), hover + (dragging * 0.5));
    float3 white = lerp(float3(0.86, 0.86, 0.86), float3(1.00, 0.98, 0.96), hover);
    float3 puckColor =
        (red * ring0)
        + (white * ring1)
        + (lerp(red, white, stripe * 0.35) * center);
    float puckAlpha = edge;

    float socketOuter = smoothstep(0.94, 0.84, radius) - smoothstep(0.84, 0.70, radius);
    float socketInner = smoothstep(0.52, 0.44, radius) - smoothstep(0.34, 0.26, radius);
    float socketCross =
        (1.0 - smoothstep(0.00, 0.08, abs(markerCoord.x))) * smoothstep(0.08, 0.40, abs(markerCoord.y));
    socketCross +=
        (1.0 - smoothstep(0.00, 0.08, abs(markerCoord.y))) * smoothstep(0.08, 0.40, abs(markerCoord.x));
    float3 socketColor =
        (float3(0.82, 0.22, 0.18) * socketOuter)
        + (float3(0.92, 0.92, 0.92) * socketInner)
        + (float3(0.86, 0.24, 0.20) * socketCross * 0.22);
    float socketAlpha = saturate((socketOuter * 0.75) + (socketInner * 0.55) + (socketCross * 0.10));

    float3 rgb = lerp(socketColor, puckColor, isPuck);
    float alpha = lerp(socketAlpha, puckAlpha, isPuck);
    float glow = 0.94 + (hover * 0.12) + (dragging * 0.14) + (0.04 * sin((radius * 18.0) - (t * 2.2)));
    return float4((rgb * color.rgb) * glow, color.a * alpha);
}

float4 apply_timeline_add_text_track_button(float2 uv, float4 color, float4 state) {
    float t = PanelTime();
    float near = saturate(state.x);
    float hover = saturate(state.y);
    float pressed = saturate(state.z);
    float click = saturate(state.w);
    float edgeDistance = abs((uv.x - 0.5) * 2.0);
    float verticalArc = 1.0 - smoothstep(0.10, 0.96, abs((uv.y - 0.48) * 2.0));
    float topRim = 1.0 - smoothstep(0.04, 0.28, uv.y);
    float engage = saturate(max(hover, near * 0.82));
    float bouncePhase = sin((t * 6.8) + ((1.0 - edgeDistance) * 8.0));
    float bounce = bouncePhase * (0.05 + (0.03 * hover));
    float greenFront = smoothstep(1.0 - engage - 0.14, 1.0 - engage + 0.02, edgeDistance + bounce);
    float greenFill = saturate(greenFront * engage);
    float frontBand =
        (1.0 - smoothstep(0.00, 0.08, abs(edgeDistance - (1.0 - engage + (bounce * 0.45)))))
        * engage;
    float sheen = 0.5 + (0.5 * sin((uv.x * 11.0) - (t * (1.9 + (hover * 0.8)))));
    float grain = 0.5 + (0.5 * sin((uv.x * 18.0) + (uv.y * 24.0) + (t * 1.4)));
    float idleGlow = 0.92 + (0.05 * sin((uv.x * 9.0) + (t * 0.9))) + (0.03 * sin((uv.y * 13.0) - (t * 1.1)));
    float settle = saturate(greenFill + (hover * 0.22) + (click * 0.18));
    float3 baseSteel = color.rgb * lerp(float3(0.92, 0.97, 1.02), float3(1.02, 1.04, 1.08), sheen * 0.45);
    float3 emeraldShadow = float3(0.08, 0.34, 0.19);
    float3 emeraldMid = float3(0.20, 0.64, 0.34);
    float3 emeraldHighlight = float3(0.58, 0.96, 0.66);
    float3 mint = float3(0.88, 1.00, 0.91);
    float3 greenTint = lerp(emeraldShadow, emeraldMid, 0.42 + (0.32 * verticalArc) + (0.18 * sheen));
    greenTint = lerp(greenTint, emeraldHighlight, (0.24 * topRim) + (0.12 * grain));
    greenTint += mint * ((frontBand * (0.18 + (0.14 * hover))) + (topRim * 0.08));
    float glow = idleGlow + (verticalArc * (0.06 + (0.06 * near))) + (topRim * (0.08 + (0.10 * settle))) + (frontBand * 0.20) - (pressed * 0.08);
    float3 rgb = lerp(baseSteel, greenTint, settle);
    return float4(rgb * glow, color.a);
}

float sd_equilateral_triangle_px(float2 pointPx, float radiusPx) {
    float k = sqrt(3.0);
    float2 p = pointPx;
    p.x = abs(p.x) - radiusPx;
    p.y = p.y + (radiusPx / k);
    if (p.x + (k * p.y) > 0.0) {
        p = float2(p.x - (k * p.y), (-k * p.x) - p.y) * 0.5;
    }
    p.x -= clamp(p.x, -2.0 * radiusPx, 0.0);
    float distancePx = length(p);
    return (p.y < 0.0) ? -distancePx : distancePx;
}

float4 apply_cursor_latency_ripple(float2 uv, float4 color, float4 state) {
    float2 originUv = state.xy;
    float2 panelPx = max(state.zw, float2(1.0, 1.0));
    float2 pointPx = (uv - originUv) * panelPx;
    float triangleRadiusPx = max(18.0, min(panelPx.x, panelPx.y) * 0.055);
    float sdf = sd_equilateral_triangle_px(pointPx, triangleRadiusPx);
    float ripple = 0.5 + (0.5 * cos(abs(sdf) * 0.165));
    float edge = 1.0 - smoothstep(0.0, triangleRadiusPx * 0.9, abs(sdf));
    float glow = 0.88 + (0.24 * ripple) + (0.16 * edge);
    float stripe = 0.94 + (0.06 * sin((uv.y * panelPx.y * 0.035) - (PanelTime() * 1.2)));
    float3 rgb = color.rgb * glow * stripe;
    return float4(rgb, color.a);
}

float4 PSMain(PsInput input) : SV_TARGET {
    if (input.effect > 11.5 && input.effect < 12.5) {
        if (transformed_text_clip_rect.z >= 0.0
            && (input.position.x < transformed_text_clip_rect.x
                || input.position.y < transformed_text_clip_rect.y
                || input.position.x >= transformed_text_clip_rect.z
                || input.position.y >= transformed_text_clip_rect.w)) {
            discard;
        }
        float2 renderCoord = input.uv;
        if (input.transformedFlag > 0.5) {
            float2 localPoint = reconstruct_transformed_local_point(input.position.xy);
            if (localPoint.x < input.localBounds.x
                || localPoint.x > input.localBounds.y
                || localPoint.y < input.localBounds.z
                || localPoint.y > input.localBounds.w) {
                discard;
            }
            renderCoord = float2(
                remap_range(localPoint.x, input.localBounds.x, input.localBounds.y, input.uvBounds.x, input.uvBounds.y),
                remap_range(localPoint.y, input.localBounds.z, input.localBounds.w, input.uvBounds.z, input.uvBounds.w)
            );
        }
        float coverage = slug_coverage(renderCoord, input.glyph, input.glyphData, input.banding);
        float4 shaded = float4(input.color.rgb, input.color.a * coverage);
        if (scene_time.y > 0.5) {
            float4 geometry = slug_geometry_debug(renderCoord, input.glyphData, input.debugId);
            shaded.rgb = lerp(shaded.rgb, geometry.rgb, geometry.a * 0.88);
            shaded.a = max(shaded.a, geometry.a * 0.82);
        }
        return premultiply_alpha(shaded);
    }

    if (input.effect > 12.5 && input.effect < 13.5) {
        float4 sprite = sample_sprite_atlas(input.uv);
        return premultiply_alpha(float4(sprite.rgb * input.color.rgb, sprite.a * input.color.a));
    }

    if (input.effect > 7.5 && input.effect < 9.5) {
        return premultiply_alpha(input.color);
    }

    float4 shaded = input.color;
    if (input.effect > 31.5) {
        shaded = apply_cursor_latency_ripple(input.uv, input.color, input.glyphData);
    } else if (input.effect > 30.5) {
        shaded = apply_timeline_add_text_track_button(input.uv, input.color, input.glyphData);
    } else if (input.effect > 29.5) {
        shaded = apply_target_marker(input.uv, input.color, input.glyphData);
    } else if (input.effect > 28.5) {
        shaded = apply_transcription_toggle(input.uv, input.color, input.glyphData);
    } else if (input.effect > 27.5) {
        shaded = apply_playback_button(input.uv, input.color, input.glyphData);
    } else if (input.effect > 26.5) {
        shaded = apply_demo_toggle(input.uv, input.color, input.glyphData);
    } else if (input.effect > 25.5) {
        shaded = apply_timeline_head_grabber(input.uv, input.color, input.glyphData);
    } else if (input.effect > 24.5) {
        shaded = apply_loopback_button(input.uv, input.color, input.glyphData);
    } else if (input.effect > 23.5) {
        shaded = apply_record_arm_button(input.uv, input.color, input.glyphData);
    } else if (input.effect > 15.5) {
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