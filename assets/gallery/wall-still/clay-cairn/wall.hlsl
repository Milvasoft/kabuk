// Clay cairn: five pastel clay stones balanced on each other in a soft studio, with two
// pebbles at their foot. The stones are ellipsoids traced exactly (no marching), so the
// picture stays sharp; shadows and the contact darkening are analytic as well.

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

static const int STONES = 7;
// Centre, radii, tilt about the view axis (radians) and colour of each stone.
static const float3 CENTRE[STONES] = {
    float3(0.00, 0.33, 0.00), float3(0.10, 0.92, 0.02), float3(-0.04, 1.44, -0.02),
    float3(0.07, 1.88, 0.00), float3(0.02, 2.24, 0.00),
    float3(-1.75, 0.15, -0.55), float3(-1.22, 0.10, -1.15)
};
static const float3 RADII[STONES] = {
    float3(1.05, 0.36, 0.90), float3(0.82, 0.29, 0.72), float3(0.62, 0.26, 0.56),
    float3(0.44, 0.21, 0.40), float3(0.27, 0.17, 0.25),
    float3(0.34, 0.16, 0.30), float3(0.21, 0.11, 0.19)
};
static const float TILT[STONES] = { 0.02, -0.05, 0.07, -0.06, 0.10, 0.12, -0.20 };
static const float3 TINT[STONES] = {
    float3(0.90, 0.56, 0.47), float3(0.62, 0.72, 0.60), float3(0.72, 0.64, 0.84),
    float3(0.95, 0.80, 0.52), float3(0.58, 0.74, 0.88),
    float3(0.76, 0.70, 0.86), float3(0.92, 0.66, 0.56)
};

float2 turn(float2 v, float a)
{
    float c = cos(a);
    float s = sin(a);
    return float2(c * v.x + s * v.y, -s * v.x + c * v.y);
}

// Distance along the ray to stone i, or -1.
float trace_stone(float3 ro, float3 rd, int i)
{
    float3 o = ro - CENTRE[i];
    o.xy = turn(o.xy, TILT[i]);
    float3 d = rd;
    d.xy = turn(d.xy, TILT[i]);
    o /= RADII[i];
    d /= RADII[i];
    float a = dot(d, d);
    float b = dot(o, d);
    float c = dot(o, o) - 1.0;
    float h = b * b - a * c;
    float t = (-b - sqrt(max(h, 0.0))) / a;
    return (h > 0.0 && t > 0.0) ? t : -1.0;
}

float3 stone_normal(float3 p, int i)
{
    float3 o = p - CENTRE[i];
    o.xy = turn(o.xy, TILT[i]);
    float3 n = o / (RADII[i] * RADII[i]);
    n.xy = turn(n.xy, -TILT[i]);
    return normalize(n);
}

// Soft shadow from stone i on a point looking towards the light (1 = lit).
float stone_shadow(float3 p, float3 l, int i, float k)
{
    float3 o = p - CENTRE[i];
    o.xy = turn(o.xy, TILT[i]);
    float3 d = l;
    d.xy = turn(d.xy, TILT[i]);
    o /= RADII[i];
    d /= RADII[i];
    float s = length(d);
    d /= s;
    float b = dot(o, d);
    float c = dot(o, o) - 1.0;
    float h = b * b - c;
    float dd = sqrt(max(0.0, 1.0 - h)) - 1.0;
    float t = -b - sqrt(max(h, 0.0));
    return (t < 0.0) ? 1.0 : smoothstep(0.0, 1.0, 2.5 * k * dd / t);
}

// How much stone i hides the sky from a point with normal n.
float stone_occlusion(float3 p, float3 n, int i)
{
    float3 di = CENTRE[i] - p;
    float r = (RADII[i].x + RADII[i].y + RADII[i].z) / 3.0;
    float l = length(di);
    float nl = dot(n, di / l);
    return saturate(nl * 0.5 + 0.5) * r * r / (l * l);
}

float3 wall(float2 px)
{
    // The cairn stands two thirds across the screen.
    float2 uv = (px - float2(0.66 * resolution.x, 0.52 * resolution.y)) / resolution.y;
    uv.y = -uv.y;
    float3 ro = float3(0.0, 2.2, -10.5);
    float3 ta = float3(-0.3, 1.05, 0.0);
    float3 fw = normalize(ta - ro);
    float3 rt = normalize(cross(float3(0.0, 1.0, 0.0), fw));
    float3 up = cross(fw, rt);
    float3 rd = normalize(uv.x * rt + uv.y * up + 2.6 * fw);

    float3 l = normalize(float3(-0.55, 0.75, -0.45));
    float3 backdrop = float3(0.93, 0.86, 0.82);
    float3 floorc = float3(0.94, 0.87, 0.83);

    // Find the nearest stone.
    float tmin = 1e9;
    int hit = -1;
    [unroll] for (int i = 0; i < STONES; i++)
    {
        float t = trace_stone(ro, rd, i);
        if (t > 0.0 && t < tmin)
        {
            tmin = t;
            hit = i;
        }
    }
    // The floor.
    float tf = -ro.y / rd.y;
    bool on_floor = (rd.y < 0.0) && (tf < tmin);

    // A studio sweep behind: lighter at the top, warm towards the floor.
    float3 c = lerp(backdrop * 0.95, float3(0.97, 0.92, 0.90), saturate(uv.y * 1.2 + 0.4));

    float3 p;
    float3 n;
    float3 albedo;
    float t = tmin;
    if (on_floor)
    {
        t = tf;
        p = ro + rd * t;
        n = float3(0.0, 1.0, 0.0);
        albedo = floorc;
    }
    else
    {
        p = ro + rd * tmin;
        n = float3(0.0, 1.0, 0.0);
        albedo = backdrop;
        [unroll] for (int j = 0; j < STONES; j++)
        {
            if (j == hit)
            {
                n = stone_normal(p, j);
                albedo = TINT[j];
            }
        }
    }

    if (on_floor || hit >= 0)
    {
        float sh = 1.0;
        float occ = 0.0;
        [unroll] for (int k = 0; k < STONES; k++)
        {
            if (k != hit)
            {
                sh = min(sh, stone_shadow(p + n * 0.002, l, k, 1.3 / softness));
                occ += stone_occlusion(p, n, k);
            }
        }
        occ = lerp(0.50, 1.0, saturate(1.0 - 0.9 * occ));
        float ndl = dot(n, l);
        // Clay: soft wrapped diffuse, a broad dull sheen and a touch of light through it.
        float dif = saturate((ndl + 0.30) / 1.30) * sh;
        float sky = 0.55 + 0.45 * n.y;
        float3 hv = normalize(l - rd);
        float spe = pow(saturate(dot(n, hv)), 24.0) * sh * 0.18;
        float fre = pow(1.0 - saturate(dot(n, -rd)), 3.0);
        float3 lin = float3(1.00, 0.95, 0.88) * dif * 0.80;
        lin += float3(0.86, 0.84, 0.92) * sky * 0.48 * occ;
        lin += float3(1.0, 0.80, 0.75) * fre * 0.12 * occ;
        // Warm light bounced up from the floor fills the undersides.
        lin += floorc * float3(1.0, 0.88, 0.80) * saturate(0.5 - 0.5 * n.y) * 0.60 * occ;
        // Fine speckles in the glaze of the stones.
        float3 cell = floor(p * 70.0);
        float sp = frac(sin(dot(cell, float3(12.9898, 78.233, 37.719))) * 43758.5453);
        float3 fc = frac(p * 70.0) - 0.5;
        float spot = (1.0 - smoothstep(0.16, 0.30, length(fc))) * step(0.90, sp);
        albedo *= (hit >= 0 && !on_floor) ? (1.0 - 0.20 * spot) : 1.0;
        float3 col = albedo * lin + spe * float3(1.0, 0.97, 0.92);
        // The floor melts into the sweep with distance.
        float fog = on_floor ? smoothstep(9.0, 30.0, t) : 0.0;
        c = lerp(col, c, fog);
    }

    float2 vg = px / resolution - 0.5;
    c *= 1.0 - 0.18 * dot(vg, vg);
    c = hue_turn(c, hue);
    c += (grain01(px) - 0.5) * (grain * 3.0 / 255.0);
    return c;
}
