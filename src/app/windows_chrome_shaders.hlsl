// windowing[impl garden-band.feathered]

static const float GARDEN_INNER_SOFTEN_PX = 2.5;
static const float GARDEN_RIM_OUTER_PX = 8.0;
static const float GARDEN_RING_OFFSET_PX = 11.5;
static const float GARDEN_RING_WIDTH_PX = 9.5;
static const float GARDEN_OUTER_FEATHER_PX = 14.0;
static const float GARDEN_CORNER_SOFTEN_PX = 10.0;

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
    return triangle_mask(uv, float2(0.30, 0.22), float2(0.74, 0.50), float2(0.30, 0.78), 0.015);
}

float icon_diagnostics(float2 uv) {
    float2 p = (uv - 0.5) * 2.0;
    float line1 = sdBox(p - float2(0.0, -0.34), float2(0.48, 0.08));
    float line2 = sdBox(p, float2(0.48, 0.08));
    float line3 = sdBox(p - float2(0.0, 0.34), float2(0.48, 0.08));
    float distance_field = min(line1, min(line2, line3));
    return 1.0 - smoothstep(0.02, 0.08, distance_field);
}

float box_fill_mask(float2 uv, float2 center, float2 halfExtents) {
    float distance_field = sdBox(uv - center, halfExtents);
    return 1.0 - smoothstep(0.0, 0.02, distance_field);
}

float box_outline_mask(float2 uv, float2 center, float2 halfExtents, float thickness) {
    float outer = box_fill_mask(uv, center, halfExtents);
    float2 innerHalfExtents = max(halfExtents - thickness.xx, 0.01.xx);
    float inner = box_fill_mask(uv, center, innerHalfExtents);
    return saturate(outer - inner);
}

float line_segment_mask(float2 uv, float2 a, float2 b, float thickness) {
    float distance_field = segment_distance(uv, a, b);
    return 1.0 - smoothstep(thickness, thickness + 0.02, distance_field);
}

float icon_minimize(float2 uv) {
    return box_fill_mask(uv, float2(0.5, 0.64), float2(0.22, 0.04));
}

float icon_maximize(float2 uv) {
    return box_outline_mask(uv, float2(0.5, 0.52), float2(0.20, 0.20), 0.05);
}

float icon_restore(float2 uv) {
    float2 halfExtents = float2(0.16, 0.16);
    float2 backCenter = float2(0.44, 0.42);
    float2 frontCenter = float2(0.58, 0.56);
    float front = box_outline_mask(uv, frontCenter, halfExtents, 0.045);
    float frontCover = box_fill_mask(uv, frontCenter, halfExtents + 0.03.xx);
    float back = box_outline_mask(uv, backCenter, halfExtents, 0.045) * (1.0 - frontCover);
    return saturate(front + back);
}

float icon_close(float2 uv) {
    float line1 = line_segment_mask(uv, float2(0.30, 0.30), float2(0.70, 0.70), 0.045);
    float line2 = line_segment_mask(uv, float2(0.70, 0.30), float2(0.30, 0.70), 0.045);
    return max(line1, line2);
}

float chrome_icon_mask(float2 uv, float effect) {
    if (effect < 16.5) {
        return icon_diagnostics(uv);
    }

    if (effect < 17.5) {
        return icon_minimize(uv);
    }

    if (effect < 18.5) {
        return icon_maximize(uv);
    }

    if (effect < 19.5) {
        return icon_restore(uv);
    }

    return icon_close(uv);
}

float hash21(float2 p) {
    p = frac(p * float2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return frac(p.x * p.y);
}

float noise2d(float2 p) {
    float2 i = floor(p);
    float2 f = frac(p);
    float2 u = f * f * (3.0 - 2.0 * f);

    float a = hash21(i);
    float b = hash21(i + float2(1.0, 0.0));
    float c = hash21(i + float2(0.0, 1.0));
    float d = hash21(i + float2(1.0, 1.0));

    return lerp(lerp(a, b, u.x), lerp(c, d, u.x), u.y);
}

float fbm2d(float2 p) {
    float value = 0.0;
    float amplitude = 0.5;

    [unroll]
    for (int octave = 0; octave < 4; octave++) {
        value += amplitude * noise2d(p);
        p = p * 2.02 + float2(17.0, 9.0);
        amplitude *= 0.5;
    }

    return saturate(value);
}

float outer_edge_distance_px(float2 uv) {
    float uv_per_pixel_x = max(abs(ddx(uv.x)) + abs(ddy(uv.x)), 1.0 / 65536.0);
    float uv_per_pixel_y = max(abs(ddx(uv.y)) + abs(ddy(uv.y)), 1.0 / 65536.0);
    return min(
        min(uv.x / uv_per_pixel_x, (1.0 - uv.x) / uv_per_pixel_x),
        min(uv.y / uv_per_pixel_y, (1.0 - uv.y) / uv_per_pixel_y)
    );
}

float inner_rect_signed_distance_px(float2 uv, float4 contentBounds) {
    float2 center = (contentBounds.xy + contentBounds.zw) * 0.5;
    float2 halfExtents = max((contentBounds.zw - contentBounds.xy) * 0.5, 0.001.xx);
    float signedDistanceUv = sdBox(uv - center, halfExtents);
    float uv_per_pixel = max(
        max(abs(ddx(uv.x)) + abs(ddy(uv.x)), abs(ddx(uv.y)) + abs(ddy(uv.y))),
        1.0 / 65536.0
    );
    return signedDistanceUv / uv_per_pixel;
}

float2 inner_rect_outside_distance_px(float2 uv, float4 contentBounds) {
    float2 center = (contentBounds.xy + contentBounds.zw) * 0.5;
    float2 halfExtents = max((contentBounds.zw - contentBounds.xy) * 0.5, 0.001.xx);
    float2 uv_per_pixel = float2(
        max(abs(ddx(uv.x)) + abs(ddy(uv.x)), 1.0 / 65536.0),
        max(abs(ddx(uv.y)) + abs(ddy(uv.y)), 1.0 / 65536.0)
    );
    float2 outside_uv = max(abs(uv - center) - halfExtents, 0.0.xx);
    return outside_uv / uv_per_pixel;
}

float4 apply_garden_frame(float2 uv, float4 color, float4 contentBounds) {
    float innerDistancePx = inner_rect_signed_distance_px(uv, contentBounds);
    if (innerDistancePx < 0.0) {
        return float4(0.0, 0.0, 0.0, 0.0);
    }

    float outerEdgePx = outer_edge_distance_px(uv);
    float outerFeather = smoothstep(0.0, GARDEN_OUTER_FEATHER_PX, outerEdgePx);
    float t = PanelTime();
    float2 flowUv = uv * 7.0 + float2(t * 0.07, -t * 0.05);
    float contour = fbm2d(flowUv + innerDistancePx * 0.03.xx);
    float ribbon = 0.5 + (0.5 * sin((innerDistancePx * 0.26) - (t * 2.1) + (contour * 6.2)));
    float innerTransition = smoothstep(0.0, GARDEN_INNER_SOFTEN_PX, innerDistancePx);
    float rim = innerTransition * (1.0 - smoothstep(GARDEN_INNER_SOFTEN_PX, GARDEN_RIM_OUTER_PX, innerDistancePx));
    float halo = exp(-abs(innerDistancePx - GARDEN_RING_OFFSET_PX) / GARDEN_RING_WIDTH_PX);
    float2 outsideDistancePx = inner_rect_outside_distance_px(uv, contentBounds);
    float cornerBlend = smoothstep(0.0, GARDEN_CORNER_SOFTEN_PX, min(outsideDistancePx.x, outsideDistancePx.y));
    float alpha = saturate((rim * 0.20) + (halo * (0.16 + (0.08 * ribbon))) + (contour * 0.04));
    alpha *= outerFeather * lerp(1.0, 0.82, cornerBlend) * color.a;

    float3 cool = lerp(color.rgb * float3(0.76, 0.84, 0.96), float3(0.34, 0.68, 0.94), contour);
    float3 warm = float3(1.00, 0.61, 0.43);
    float3 glow = lerp(cool, warm, saturate((halo * 0.34) + (ribbon * 0.06)));
    glow += color.rgb * (rim * 0.05);
    glow += warm * (halo * 0.05 * ribbon);
    glow *= lerp(1.0, 0.88, cornerBlend);

    return float4(glow, alpha);
}

float4 apply_window_chrome_button(float2 uv, float4 color, float4 state, float effect) {
    float t = PanelTime();
    float near = state.x;
    float hover = state.y;
    float pressed = state.z;
    float click = state.w;
    float center = 1.0 - smoothstep(0.0, 0.80, distance(uv, float2(0.5, 0.46)));
    float rim = 1.0 - smoothstep(0.18, 0.5, abs(uv.y - 0.08));
    float sheen = 0.5 + (0.5 * sin((uv.x * 10.0) + (uv.y * 8.0) - (t * (0.7 + hover))));
    float intensity = 0.88 + (near * 0.04) + (hover * 0.08) + (center * (0.06 + (0.05 * hover))) + (click * 0.10) + (sheen * 0.03) - (pressed * 0.08);
    float3 tint = color.rgb * lerp(float3(0.90, 0.92, 0.98), float3(1.02, 1.04, 1.08), hover + (click * 0.30));
    float topGlow = rim * (0.04 + (0.08 * hover) + (0.06 * click));
    float3 shaded = tint * (intensity + topGlow);
    float iconMask = chrome_icon_mask(uv, effect);
    float3 iconColor = float3(0.95, 0.96, 0.99) * (0.94 + (0.08 * hover) + (0.06 * click));
    shaded = lerp(shaded, iconColor, iconMask);
    return float4(shaded, color.a);
}