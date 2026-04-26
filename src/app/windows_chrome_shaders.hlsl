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

float sdRoundedBox(float2 p, float2 b, float r) {
    float2 q = abs(p) - b + r.xx;
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
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

float icon_pin(float2 uv) {
    float head = box_fill_mask(uv, float2(0.50, 0.30), float2(0.18, 0.055));
    float neck = line_segment_mask(uv, float2(0.50, 0.34), float2(0.50, 0.58), 0.045);
    float tip = triangle_mask(uv, float2(0.38, 0.55), float2(0.62, 0.55), float2(0.50, 0.76), 0.018);
    float shine = line_segment_mask(uv, float2(0.42, 0.24), float2(0.58, 0.24), 0.025);
    return saturate(max(max(head, neck), max(tip, shine)));
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

float ring_mask(float2 uv, float2 center, float radius, float thickness) {
    float distance_field = abs(length(uv - center) - radius);
    return 1.0 - smoothstep(thickness, thickness + 0.018, distance_field);
}

float icon_gear(float2 uv) {
    float2 p = uv - 0.5;
    float angle = atan2(p.y, p.x);
    float tooth_wave = abs(cos(angle * 8.0));
    float tooth_radius = 0.31 + (0.075 * smoothstep(0.62, 0.96, tooth_wave));
    float outer = 1.0 - smoothstep(tooth_radius, tooth_radius + 0.018, length(p));
    float inner_clear = 1.0 - smoothstep(0.125, 0.145, length(p));
    float body = saturate(outer - inner_clear);
    float ring = ring_mask(uv, float2(0.5, 0.5), 0.22, 0.04);
    return saturate(max(body, ring));
}

float chrome_icon_mask(float2 uv, float effect) {
    if (effect < 16.5) {
        return icon_pin(uv);
    }

    if (effect < 17.5) {
        return icon_diagnostics(uv);
    }

    if (effect < 18.5) {
        return icon_minimize(uv);
    }

    if (effect < 19.5) {
        return icon_maximize(uv);
    }

    if (effect < 20.5) {
        return icon_restore(uv);
    }

    if (effect > 21.5) {
        return icon_gear(uv);
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

float4 apply_record_arm_button(float2 uv, float4 color, float4 state) {
    float t = PanelTime();
    float recording = state.x;
    float armed = state.y;
    float2 p = uv - 0.5;
    float radius = length(p);
    float circle = 1.0 - smoothstep(0.24, 0.30, radius);
    float rim = 1.0 - smoothstep(0.30, 0.38, radius);
    float pulse = 0.5 + (0.5 * sin(t * 5.6));
    float glow = recording * (0.22 + 0.34 * pulse) * exp(-pow(max(radius - 0.20, 0.0) / 0.16, 2.0));
    float soft = exp(-pow(radius / 0.34, 4.0));
    float3 inactiveRed = float3(0.32, 0.04, 0.035);
    float3 armedRed = float3(0.55, 0.055, 0.045);
    float3 hotRed = float3(1.0, 0.10, 0.07);
    float3 shaded = lerp(inactiveRed, armedRed, armed);
    shaded = lerp(shaded, hotRed, recording * (0.75 + 0.25 * pulse));
    shaded += hotRed * (glow * 0.72);
    shaded += color.rgb * (soft * 0.06);
    float alpha = saturate((circle * 0.96) + (rim * (0.18 + recording * 0.20)) + (glow * 0.48));
    return float4(shaded, alpha * color.a);
}

float loopback_icon(float2 uv) {
    float2 left = uv - float2(0.36, 0.50);
    float speaker = (1.0 - smoothstep(0.10, 0.13, abs(left.x))) * (1.0 - smoothstep(0.18, 0.21, abs(left.y)));
    float cone = saturate(1.0 - abs((uv.x - 0.50) - (abs(uv.y - 0.50) * 0.9)) * 14.0) * step(0.39, uv.x) * step(uv.x, 0.58);
    float2 ring_center = uv - float2(0.58, 0.50);
    float ring1 = abs(length(ring_center) - 0.12);
    float ring2 = abs(length(ring_center) - 0.19);
    float waves = (1.0 - smoothstep(0.01, 0.03, ring1)) + (1.0 - smoothstep(0.01, 0.03, ring2));
    return saturate(speaker + cone + waves);
}

float4 apply_loopback_button(float2 uv, float4 color, float4 state) {
    float enabled = state.x;
    float hover = state.y;
    float pressed = state.z;
    float t = PanelTime();
    float2 p = uv - 0.5;
    float radius = length(p);
    float plate = 1.0 - smoothstep(0.42, 0.50, radius);
    float rim = 1.0 - smoothstep(0.32, 0.44, radius);
    float sweep = 0.5 + (0.5 * sin((uv.x * 9.0) - (t * (0.9 + enabled * 1.6))));
    float shimmer = enabled * (0.5 + 0.5 * sin((uv.y * 12.0) + (t * 2.1)));
    float intensity = 0.82 + (hover * 0.10) + (enabled * 0.18) + (sweep * 0.06) + (shimmer * 0.10) - (pressed * 0.08);
    float3 base = lerp(float3(0.20, 0.30, 0.28), float3(0.26, 0.60, 0.52), enabled);
    float3 shaded = base * intensity;
    shaded += float3(0.74, 0.95, 0.88) * (rim * (0.08 + enabled * 0.16));
    float icon = loopback_icon(uv);
    shaded = lerp(shaded, float3(0.92, 0.98, 0.96), icon * (0.78 + enabled * 0.22));
    float alpha = saturate((plate * 0.94) + (rim * 0.18));
    return float4(shaded, alpha * color.a);
}

float4 apply_timeline_head_grabber(float2 uv, float4 color, float4 state) {
    float active = state.x;
    float hover = state.y;
    float grabbed = state.z;
    float kind = state.w;
    float t = PanelTime();
    float2 p = abs(uv - 0.5);
    float box = (1.0 - smoothstep(0.34, 0.42, max(p.x, p.y))) * (1.0 - smoothstep(0.40, 0.50, length(uv - 0.5)));
    float bevel = 1.0 - smoothstep(0.18, 0.46, max(p.x, p.y));
    float scan = 0.5 + (0.5 * sin((uv.y * 18.0) + (t * (1.0 + grabbed)) + kind));
    float3 shaded = color.rgb * (0.82 + (active * 0.10) + (hover * 0.10) + (grabbed * 0.18) + (scan * 0.04));
    shaded += float3(0.94, 0.96, 0.99) * (bevel * (0.08 + hover * 0.10 + grabbed * 0.12));
    float alpha = saturate((box * 0.98) + (bevel * 0.10));
    return float4(shaded, alpha * color.a);
}

float4 apply_demo_toggle(float2 uv, float4 color, float4 state) {
    float enabled = state.x;
    float hover = state.y;
    float pressed = state.z;
    float transition = state.w;
    float t = PanelTime();
    float2 p = uv - 0.5;
    float capsule = 1.0 - smoothstep(0.40, 0.50, sdRoundedBox(p, float2(0.46, 0.28), 0.25));
    float rim = 1.0 - smoothstep(0.30, 0.48, abs(p.y));
    float scan = 0.5 + (0.5 * sin((uv.x * 16.0) - (t * (1.0 + enabled * 1.4))));
    float sparkle = 0.5 + (0.5 * sin((uv.y * 20.0) + (uv.x * 8.0) + (t * 2.2)));
    float3 offColor = float3(0.30, 0.11, 0.14);
    float3 onColor = float3(0.12, 0.62, 0.52);
    float3 stateColor = lerp(offColor, onColor, enabled);
    float sweepCenter = lerp(0.22, 0.78, enabled);
    float sweep = exp(-pow((uv.x - sweepCenter) / 0.20, 2.0)) * transition;
    float intensity = 0.78 + (hover * 0.12) + (scan * 0.07) + (sparkle * 0.04) + (sweep * 0.25) - (pressed * 0.10);
    float3 shaded = stateColor * intensity;
    shaded += lerp(float3(1.00, 0.36, 0.34), float3(0.70, 1.00, 0.92), enabled) * (rim * (0.11 + hover * 0.08 + transition * 0.18));
    shaded += color.rgb * 0.04;
    float alpha = saturate((capsule * 0.96) + (rim * 0.10));
    return float4(shaded, alpha * color.a);
}