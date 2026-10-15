// Isometric isle: a little square world of cubes, cut out of the ground like a diorama
// and seen from the classic isometric angle. Hills, beaches, a lake and blocky trees sit
// on a slab whose cut sides show the layers of earth. Rays walk the grid cell by cell.

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

float3 hue_turn(float3 c, float a)
{
    float3 k = float3(0.57735, 0.57735, 0.57735);
    float t = a * 6.2831853;
    float ct = cos(t);
    return c * ct + cross(k, c) * sin(t) + k * dot(k, c) * (1.0 - ct);
}

static const float N = 18.0;      // cells along each side of the slab
static const float FLOOR = -3.0;  // the bottom of the slab
static const float WATER = 0.55;  // the water's surface

// A column: its top (.x) and kind (.y: 0 water, 1 sand, 2 grass, 3 forest, 4 rock,
// 5 snow, 6 tree).
float2 column(float2 cell)
{
    float2 c = (cell + 0.5) / N * 2.0 - 1.0;
    float2 o = float2(seed * 3.7, seed * 1.9);
    // A few crossing waves are cheaper than noise and enough for a small world.
    float n = 0.62 + 0.30 * sin(cell.x * 0.47 + o.x) * cos(cell.y * 0.41 + o.y)
        + 0.18 * sin((cell.x + cell.y) * 0.33 + o.x * 0.7) + 0.12 * cos((cell.x - cell.y) * 0.71 + o.y * 1.3);
    float edge = max(abs(c.x), abs(c.y));
    float h = 7.6 * (1.0 - 0.80 * length(c * float2(1.0, 1.15))) + 4.5 * (n - 0.85);
    // The shore keeps a ring of water inside the slab's edge.
    h = min(h, (0.86 - edge) * 22.0);
    float lvl = floor(clamp(h, 0.0, 7.0));
    if (lvl < 1.0)
    {
        return float2(WATER, 0.0);
    }
    float kind = (lvl < 2.0) ? 1.0 : ((lvl < 4.0) ? 2.0 : ((lvl < 5.0) ? 3.0 : ((lvl < 6.0) ? 4.0 : 5.0)));
    // Trees on the grass and in the forest.
    float th = hash12(cell + o * 7.0);
    bool tree = (kind == 2.0 && th < 0.14) || (kind == 3.0 && th < 0.45);
    return tree ? float2(lvl + 1.0, 6.0) : float2(lvl, kind);
}

float3 kind_colour(float kind)
{
    float3 c = float3(0.40, 0.74, 0.88);
    c = (kind == 1.0) ? float3(0.97, 0.87, 0.64) : c;
    c = (kind == 2.0) ? float3(0.60, 0.83, 0.46) : c;
    c = (kind == 3.0) ? float3(0.42, 0.68, 0.40) : c;
    c = (kind == 4.0) ? float3(0.66, 0.62, 0.66) : c;
    c = (kind == 5.0) ? float3(0.97, 0.97, 1.00) : c;
    c = (kind == 6.0) ? float3(0.27, 0.56, 0.36) : c;
    return c;
}

float3 wall(float2 px)
{
    // The slab's centre sits two thirds across the screen.
    float2 uv = (px - float2(0.64 * resolution.x, 0.47 * resolution.y)) / resolution.y;
    uv.y = -uv.y;
    float scale = 27.0;

    // An orthographic camera looking down the isometric diagonal.
    float3 rd = normalize(float3(-1.0, -0.82, -1.0));
    float3 rt = normalize(float3(1.0, 0.0, -1.0));
    float3 up = cross(rt, rd);
    float3 centre = float3(N * 0.5, 0.5, N * 0.5);
    float3 ro = centre + (rt * uv.x + up * uv.y) * scale - rd * 60.0;

    // Background: a soft pastel sky with a glow behind the slab and its shadow below.
    float2 bq = px / resolution;
    float3 c = lerp(float3(0.99, 0.88, 0.80), float3(0.80, 0.82, 0.96), smoothstep(0.0, 1.0, bq.y * 0.8 + bq.x * 0.2));
    c += float3(0.06, 0.05, 0.03) * exp(-dot(uv, uv) * 3.0);
    float2 sq = (uv - float2(0.0, -0.38)) / float2(0.42, 0.06);
    c *= 1.0 - 0.10 * exp(-dot(sq, sq) * 1.5);

    // Enter the slab's box.
    float3 bmin = float3(0.0, FLOOR, 0.0);
    float3 bmax = float3(N, 8.05, N);
    float3 inv = 1.0 / rd;
    float3 t0 = (bmin - ro) * inv;
    float3 t1 = (bmax - ro) * inv;
    float3 tn = min(t0, t1);
    float3 tf = max(t0, t1);
    float tin = max(max(tn.x, tn.y), tn.z);
    float tout = min(min(tf.x, tf.y), tf.z);
    if (tin < tout)
    {
        // Which face of the box the ray came in through: 0 = x, 1 = y (top), 2 = z.
        float axis = (tin == tn.x) ? 0.0 : ((tin == tn.z) ? 2.0 : 1.0);
        float3 p = ro + rd * (tin + 1e-4);
        float2 cell = clamp(floor(p.xz), 0.0, N - 1.0);
        float2 stepv = float2(-1.0, -1.0);
        float2 tmax = (cell - p.xz) / rd.xz;
        float2 tdel = 1.0 / abs(rd.xz);
        float t = 0.0;
        bool hit = false;
        float3 n = float3(0.0, 1.0, 0.0);
        float hy = 0.0;
        float kind = 0.0;
        float3 hp = p;
        [unroll] for (int i = 0; i < 28; i++)
        {
            if (!hit && cell.x >= 0.0 && cell.y >= 0.0)
            {
                float2 col = column(cell);
                float tx = min(tmax.x, tmax.y);
                float y0 = p.y + rd.y * t;
                float y1 = p.y + rd.y * tx;
                if (y0 <= col.x)
                {
                    // Into the column's side.
                    hit = true;
                    n = (axis == 0.0) ? float3(1.0, 0.0, 0.0) : float3(0.0, 0.0, 1.0);
                    hp = p + rd * t;
                    hy = col.x;
                    kind = col.y;
                }
                else if (y1 <= col.x)
                {
                    // Onto the column's top.
                    hit = true;
                    n = float3(0.0, 1.0, 0.0);
                    hp = p + rd * ((col.x - p.y) / rd.y);
                    hy = col.x;
                    kind = col.y;
                }
                else
                {
                    bool goes_x = tmax.x < tmax.y;
                    t = tx;
                    axis = goes_x ? 0.0 : 2.0;
                    cell += goes_x ? float2(-1.0, 0.0) : float2(0.0, -1.0);
                    tmax += goes_x ? float2(tdel.x, 0.0) : float2(0.0, tdel.y);
                }
            }
        }
        if (hit)
        {
            float3 albedo = kind_colour(kind);
            float2 fc = frac(hp.xz);
            float3 sun = normalize(float3(-0.35, 0.85, 0.45));
            float shade = 0.62 + 0.38 * saturate(dot(n, sun));
            shade = (n.x > 0.5) ? 0.80 : shade;
            shade = (n.z > 0.5) ? 0.64 : shade;
            float3 col = albedo;
            if (n.y < 0.5)
            {
                // Sides: the top block keeps its colour, then layers of earth below.
                float depth = hy - hp.y;
                float3 soil = lerp(float3(0.66, 0.48, 0.36), float3(0.50, 0.36, 0.30), smoothstep(0.0, 3.0, depth));
                soil = lerp(soil, float3(0.58, 0.56, 0.58), step(hy - 0.0, 0.0) + step(3.0, depth) * 0.6);
                float3 water_side = lerp(float3(0.42, 0.72, 0.88), float3(0.30, 0.52, 0.74), smoothstep(0.0, 2.0, depth));
                bool wet = (kind == 0.0);
                bool leafy = (kind == 6.0);
                float topband = 1.0 - step(leafy ? 1.0 : 0.25, depth);
                col = wet ? lerp(water_side, soil, step(0.6, depth)) : lerp(soil, albedo, topband);
                // Faint lines between the cube layers.
                float lay = frac(hp.y);
                col *= 1.0 - 0.06 * (1.0 - smoothstep(0.0, 0.05, min(lay, 1.0 - lay)));
            }
            else
            {
                // Tops: a soft bevel where cubes meet, and sparkle on the water.
                float bev = min(min(fc.x, 1.0 - fc.x), min(fc.y, 1.0 - fc.y));
                col *= (kind == 0.0) ? 1.0 : (0.94 + 0.06 * smoothstep(0.0, 0.08, bev));
                float ripple = sin((hp.x + hp.z) * 9.0 + sin(hp.x * 3.0) * 2.0);
                col += (kind == 0.0) ? float3(0.10, 0.10, 0.08) * smoothstep(0.85, 1.0, ripple) : float3(0.0, 0.0, 0.0);
                // A higher neighbour towards the sun casts a soft shadow.
                float2 nb = column(floor(hp.xz) + float2(-1.0, 0.0)).xy;
                float2 nb2 = column(floor(hp.xz) + float2(0.0, 1.0)).xy;
                float occ = saturate(nb.x - hy) * (1.0 - smoothstep(0.0, 0.7, fc.x)) + saturate(nb2.x - hy) * smoothstep(0.3, 1.0, fc.y);
                col *= 1.0 - 0.22 * saturate(occ);
            }
            c = col * shade;
        }
    }

    c = hue_turn(c, hue);
    c += (grain01(px) - 0.5) * (grain * 3.0 / 255.0);
    return c;
}
