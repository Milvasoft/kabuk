// Fireflies over the meadow: two faint lines of rolling ground, and fireflies drifting
// above the near one, each flashing in its own slow rhythm. The grass is never drawn by
// itself: its blades only show where a flash lights them.
// The air above the ground is cut into cells that slide sideways, each row at its own
// pace; a cell holds at most one fly, so a pixel only looks at the cells around it.

static const float CELL_W = 0.11;
static const float CELL_H = 0.075;
static const int ROWS = 4;

float hash12(float2 p)
{
    float3 p3 = frac(float3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
}

float ground(float x)
{
    return 0.29 + 0.022 * sin(1.3 * x + 0.7) + 0.011 * sin(3.4 * x + 2.1) + 0.004 * sin(9.0 * x);
}

// The grass: a blade in each narrow cell, rooted on the ground and leaning; how much of
// the pixel it covers.
float grass(float2 uv, float pix)
{
    float cell = 0.0062;
    float c0 = floor(uv.x / cell);
    float cover = 0;
    [unroll]
    for (int k = -1; k <= 1; k++)
    {
        float c = c0 + k;
        float h1 = hash12(float2(c, 3.1));
        float h2 = hash12(float2(c, 7.7));
        float rootX = (c + 0.2 + 0.6 * h1) * cell;
        float rootY = ground(rootX) + 0.01;
        float height = 0.025 + 0.075 * h2 * h2;
        float lean = (h1 - 0.5) * 0.9;
        float s = saturate((rootY - uv.y) / height);          // 0 at the root, 1 at the tip
        float bx = rootX + lean * height * s * s;              // a gentle curve
        float halfW = 0.0011 * (1.0 - s) + 0.5 * pix;
        float inside = step(uv.y, rootY) * step(rootY - height, uv.y);
        cover = max(cover, inside * saturate((halfW - abs(uv.x - bx)) / pix + 0.5));
    }
    return cover;
}

float3 wall(float2 px)
{
    float2 uv = (px - 0.5 * resolution) / resolution.y;
    float pix = 1.0 / resolution.y;
    float t = time * drift;
    float g = ground(uv.x);
    float up = g - uv.y;                     // height above the near ground

    float3 glowCol = float3(0.8, 1.0, 0.34);
    float3 light = 0;
    float shine = 0;

    float row0 = floor(up / CELL_H);
    [unroll]
    for (int dy = -1; dy <= 1; dy++)
    {
        float row = row0 + dy;
        float inBand = step(0.0, row) * step(row, ROWS - 1.0);
        float pace = (0.006 + 0.01 * hash12(float2(row, 1.7))) * (fmod(abs(row), 2.0) < 0.5 ? 1.0 : -1.0);
        float sx = uv.x + t * pace;
        float col0 = floor(sx / CELL_W);
        [unroll]
        for (int dx = -1; dx <= 1; dx++)
        {
            float2 id = float2(col0 + dx, row);
            float a = hash12(id + 0.13);
            float b = hash12(id + 7.31);
            float c = hash12(id + 3.77);
            float e = hash12(id + 9.91);
            float live = step(e, swarm) * inBand;
            float depth = 0.45 + 0.55 * c;
            // The fly's place: its cell, plus a slow wander inside it.
            float fx = (id.x + 0.5 + 0.28 * sin(t * (0.21 + 0.2 * c) + 6.28 * b)) * CELL_W;
            float fy = (row + 0.5 + 0.3 * sin(t * (0.33 + 0.25 * a) + 6.28 * c)) * CELL_H;
            float2 d = float2(sx - fx, up - fy);
            // A flash: a quick rise, a slower fade, then a faint glow until the next one.
            float period = 3.5 + 5.0 * a;
            float ph = frac(time / period + c);
            float flash = smoothstep(0.0, 0.04, ph) * (1.0 - smoothstep(0.04, 0.42, ph));
            flash = (flash * flash + 0.05) * live * depth;
            float r2 = dot(d, d);
            float size2 = 0.000014 * depth * depth;
            float core = size2 / (r2 + size2);
            float halo = 0.0005 * depth / (r2 + 0.0005 * depth) * saturate(1.0 - r2 / 0.012);
            light += glowCol * flash * (core * core * 1.3 + 0.24 * halo * halo);
            shine += flash * halo;
        }
    }

    float rim = 0.12 * saturate(1.0 - abs(uv.y - g - 0.01) / (1.5 * pix));
    float far = 0.17 + 0.03 * sin(0.8 * uv.x + 2.6) + 0.012 * sin(2.3 * uv.x + 0.4);
    rim += 0.06 * saturate(1.0 - abs(uv.y - far) / (1.2 * pix));
    float blades = grass(uv, pix);
    float3 meadow = float3(0.25, 0.55, 0.3) * (blades * saturate(shine * 1.5) * 0.9) + float3(0.1, 0.22, 0.16) * rim;
    return meadow + light;
}
