// Light ribbons: three translucent bands of light sweep up from the lower left and turn
// over as they go. Where a band turns edge-on the light gathers into a bright fold; the
// bands add up like light, over a deep night blue.

uint pcg(uint v)
{
    uint s = v * 747796405u + 2891336453u;
    uint w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

float grain01(float2 px)
{
    return float(pcg(uint(px.x) + pcg(uint(px.y)))) * (1.0 / 4294967295.0);
}

float3 hue_turn(float3 c, float a)
{
    float3 k = float3(0.57735, 0.57735, 0.57735);
    float t = a * 6.2831853;
    float ct = cos(t);
    return c * ct + cross(k, c) * sin(t) + k * dot(k, c) * (1.0 - ct);
}

// One ribbon's light at a point. s runs 0-1 across the screen, y down in screen heights.
float3 ribbon(float s, float y, float pix, float ph, float3 ca, float3 cb, float lift)
{
    float e = smoothstep(0.0, 1.0, s);
    float centre = 0.98 - (0.62 + lift) * e + 0.07 * sin(s * 5.5 + ph) + 0.03 * sin(s * 11.0 + ph * 2.0);
    float width = 0.035 + 0.13 * e;
    float turn = s * 6.0 + ph;
    float ct = cos(turn);
    float half_w = width * abs(ct) + 0.0015;
    float d = abs(y - centre);
    float inside = 1.0 - smoothstep(half_w - pix, half_w + pix, d);
    float t = saturate(d / half_w);
    // Seen edge-on, a band's light gathers; seen flat, it spreads thin.
    float gather = 0.45 + 1.6 / (1.0 + 14.0 * abs(ct));
    // Fine threads along the band, and brighter edges.
    float threads = 0.85 + 0.15 * cos(t * 3.14159 * 18.0);
    float edge = pow(t, 6.0);
    float3 col = lerp(ca, cb, saturate(s * 1.1 + 0.25 * sin(turn) * (1.0 - t)));
    float body = inside * gather * threads * (0.55 + 0.9 * edge) * 1.6;
    // A soft halo spills around it.
    float halo = exp(-max(d - half_w, 0.0) * 14.0) * 0.22 + exp(-max(d - half_w, 0.0) * 120.0) * 0.35;
    // The band fades in from the lower left so the icon column stays dark.
    float fade = smoothstep(0.02, 0.30, s);
    return col * (body + halo * (1.0 - inside)) * fade;
}

float3 wall(float2 px)
{
    float pix = 1.0 / resolution.y;
    float s = px.x / resolution.x;
    float y = px.y / resolution.y;

    float2 q = px / resolution - float2(0.75, 0.35);
    float3 c = float3(0.012, 0.014, 0.040) + float3(0.05, 0.03, 0.10) * exp(-dot(q, q) * 5.0);

    c += ribbon(s, y, pix, 0.4, float3(0.42, 0.22, 1.00), float3(1.00, 0.36, 0.62), 0.00) * glow;
    c += ribbon(s, y, pix, 2.1, float3(0.95, 0.30, 0.55), float3(1.00, 0.66, 0.34), 0.08) * glow * 0.85;
    c += ribbon(s, y, pix, 4.0, float3(0.20, 0.55, 1.00), float3(0.55, 0.35, 1.00), -0.10) * glow * 0.75;

    // Soft roll-off of bright light instead of a hard clip.
    c = 1.0 - exp(-c * 1.15);

    c = hue_turn(c, hue);
    c += (grain01(px) - 0.5) * (grain * 3.0 / 255.0);
    return c;
}
