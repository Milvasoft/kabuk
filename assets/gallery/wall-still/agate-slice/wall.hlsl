// Agate slice: a polished cross-section of a blue agate lying on dark slate. Its bands
// follow the stone's outline inwards, wavering as the mineral did, until they open into
// a hollow lined with small quartz crystals that catch the light.

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

float hash12(float2 p)
{
    float3 p3 = frac(float3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
}

float2 hash22(float2 p)
{
    float3 p3 = frac(float3(p.xyx) * float3(0.1031, 0.1030, 0.0973));
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.xx + p3.yz) * p3.zy);
}

float noise2(float2 p)
{
    float2 i = floor(p);
    float2 f = frac(p);
    float2 u = f * f * (3.0 - 2.0 * f);
    float a = hash12(i);
    float b = hash12(i + float2(1.0, 0.0));
    float c = hash12(i + float2(0.0, 1.0));
    float d = hash12(i + float2(1.0, 1.0));
    return lerp(lerp(a, b, u.x), lerp(c, d, u.x), u.y);
}

float fbm(float2 p)
{
    float v = 0.0;
    float a = 0.5;
    [unroll] for (int i = 0; i < 4; i++)
    {
        v += a * noise2(p);
        p = p * 2.03 + float2(1.7, 9.2);
        a *= 0.5;
    }
    return v;
}

float3 hue_turn(float3 c, float a)
{
    float3 k = float3(0.57735, 0.57735, 0.57735);
    float t = a * 6.2831853;
    float ct = cos(t);
    return c * ct + cross(k, c) * sin(t) + k * dot(k, c) * (1.0 - ct);
}

// How far inside the stone a point is: 0 at the rim, growing inwards.
float depth_in(float2 q)
{
    float ang = atan2(q.y, q.x);
    float rim = 0.37 * (1.0 + 0.07 * sin(ang * 2.0 + 0.6) + 0.04 * sin(ang * 5.0 + 1.9));
    return (rim - length(q * float2(1.0, 1.12))) / 0.37 + 0.09 * (fbm(q * 5.0 + 3.0) - 0.5);
}

float3 wall(float2 px)
{
    float aspect = resolution.x / resolution.y;
    float pix = 1.0 / resolution.y;
    float2 p = px / resolution.y;
    float2 q = p - float2(aspect - 0.62, 0.50);

    // Slate: a dark cool stone with a faint cloudy grain.
    float sl = fbm(p * 7.0);
    float3 c = float3(0.085, 0.092, 0.108) * (0.75 + 0.45 * sl);
    float2 lg = p - float2(aspect - 0.45, 0.25);
    c += float3(0.020, 0.022, 0.028) * exp(-dot(lg, lg) * 2.0);

    // The slice's soft shadow on the slate.
    float sh = depth_in(q - float2(0.018, -0.026));
    c *= 1.0 - 0.55 * smoothstep(-0.10, 0.04, sh);

    float v = depth_in(q);
    if (v > -0.02)
    {
        // Bands: their spacing wavers, a few are bright chalcedony lines.
        float w = v + 0.030 * (fbm(q * 11.0) - 0.5);
        float k = bands * 40.0;
        float tone = 0.5 + 0.5 * sin(w * k + 2.6 * sin(w * 7.0 + 0.8));
        float3 indigo = float3(0.06, 0.10, 0.30);
        float3 sky = float3(0.32, 0.54, 0.80);
        float3 ice = float3(0.86, 0.91, 0.96);
        float3 teal = float3(0.12, 0.40, 0.50);
        float3 s = lerp(indigo, sky, smoothstep(0.15, 0.85, tone));
        s = lerp(s, teal, 0.5 * smoothstep(0.55, 1.0, sin(w * 9.0 + 1.0)));
        float fine = smoothstep(0.90, 0.99, sin(w * k * 2.7 + 1.3 + 3.0 * sin(w * 5.0)));
        s = lerp(s, ice, fine * 0.75);
        float dark = smoothstep(0.93, 0.995, sin(w * k * 3.1 + 0.4 + 2.0 * sin(w * 13.0)));
        s *= 1.0 - 0.35 * dark;
        // Fine growth lines everywhere.
        s *= 0.92 + 0.08 * sin(w * k * 6.3 + 2.0 * sin(w * 23.0));
        s *= 0.95 + 0.05 * sin(w * k * 17.0);
        // Towards the hollow the stone clears into pale, glassy chalcedony.
        s = lerp(s, lerp(sky, ice, 0.6), smoothstep(0.50, 0.60, w) * 0.8);
        // The outer rind and a white chalcedony skin under it.
        s = lerp(s, ice, smoothstep(0.035, 0.045, w) * (1.0 - smoothstep(0.065, 0.08, w)));
        s = lerp(float3(0.30, 0.22, 0.18) * (0.8 + 0.4 * fbm(q * 30.0)), s, smoothstep(0.028, 0.036, w));

        // The hollow: quartz crystals, each a facet of its own brightness.
        float hollow = smoothstep(0.60, 0.62, w);
        float2 g = q * 115.0;
        float2 gi = floor(g);
        float best = 9.0;
        float second = 9.0;
        float2 bid = gi;
        [unroll] for (int j = -1; j <= 1; j++)
        {
            [unroll] for (int i = -1; i <= 1; i++)
            {
                float2 cell = gi + float2(i, j);
                float2 o = cell + hash22(cell);
                float d = length(g - o);
                bool closer = d < best;
                second = closer ? best : min(second, d);
                bid = closer ? cell : bid;
                best = closer ? d : best;
            }
        }
        float facet = hash12(bid * 1.31);
        float edge = smoothstep(0.0, 0.12, second - best);
        // Each crystal is a little pyramid: lit on one side, shaded on the other.
        float2 tip = bid + hash22(bid);
        float2 to = g - tip;
        float lit = 0.5 + 0.5 * dot(to / max(length(to), 1e-3), normalize(float2(-0.7, -0.7))) * smoothstep(0.0, 0.3, length(to));
        float3 quartz = lerp(float3(0.42, 0.50, 0.72), float3(0.95, 0.97, 1.0), lit * (0.6 + 0.4 * facet)) * (0.7 + 0.3 * edge);
        // Deeper in the hollow is darker; some facets flash.
        quartz *= lerp(1.0, 0.55, smoothstep(0.62, 0.95, w));
        quartz += float3(1.0, 1.0, 1.0) * step(0.94, facet) * smoothstep(0.75, 1.0, lit) * 0.6;
        s = lerp(s, quartz, hollow);

        // Polish: a soft sheen across the slice and a lit cut edge.
        float sheen = exp(-pow(dot(q, normalize(float2(1.0, 0.8))) + 0.05, 2.0) * 30.0);
        s += float3(0.10, 0.11, 0.12) * sheen;
        float lip = 1.0 - smoothstep(0.0, 0.012, w);
        s += float3(0.16, 0.14, 0.12) * lip;

        float cover = smoothstep(-pix / 0.37 * 1.5, pix / 0.37 * 1.5, v);
        c = lerp(c, s, cover);
    }

    float2 vg = px / resolution - 0.5;
    c *= 1.0 - 0.3 * dot(vg, vg);
    c = hue_turn(c, hue);
    c += (grain01(px) - 0.5) * (grain * 3.0 / 255.0);
    return c;
}
