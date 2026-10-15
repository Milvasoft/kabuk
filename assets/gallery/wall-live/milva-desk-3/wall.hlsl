// Milva constellation: the Milvasoft eye drawn in the night sky as a constellation, gold stars
// at its pads, joints and along its half-disc, joined by fine lines the way a star chart joins
// them. The figure turns very slowly in depth like a card held up to the light, and the sky
// behind it wheels around a pole far above the screen. Sizes are in "screen units" (pixels of a 1080-line screen), so it holds at any
// resolution and aspect ratio.

static const float TAU = 6.2831853;
static const float3 MILVA_YELLOW = float3(0.984, 0.769, 0.098);
static const float3 SKY_HIGH = float3(0.008, 0.012, 0.028);
static const float3 SKY_LOW = float3(0.026, 0.031, 0.056);
static const float3 HAZE_WARM = float3(0.050, 0.034, 0.015);
static const float3 HAZE_COOL = float3(0.010, 0.016, 0.034);
static const float3 STAR_WHITE = float3(0.80, 0.85, 1.00);
static const float3 STAR_GOLD = float3(1.00, 0.84, 0.54);
static const float3 LINE_GOLD = float3(0.95, 0.76, 0.42);
static const float DITHER = 1.5 / 255.0;

// Placement, in screen heights.
static const float UNITS = 1080.0;
static const float RIGHT_SPACE = 0.62;   // from the right edge on 16:9 to 16:10
static const float CENTRE_MAX = 0.75;    // at most this far right of the middle
static const float CENTRE_Y = 0.46;
static const float HAZE_REACH = 0.40;

// The eye, in logo units (crates/kabuk-scenes/src/milva.rs).
static const float BAR_ANGLE = -0.14137; // -8.1 degrees
static const float RING_R = 13.0;
static const float MARK_SCALE = 2.5;     // screen units per logo unit

// Its turn in depth.
static const float CAMERA = 900.0;       // logo units from the eye to the figure
static const float YAW_MAX = 0.30;
static const float YAW_SECONDS = 140.0;
static const float PITCH_MAX = 0.12;
static const float PITCH_SECONDS = 97.0;

// Lines and stars, in screen units.
static const float LINE_HALF = 0.6;
static const float LINE_LEVEL = 0.32;
static const float LINE_HALO = 3.0;
static const float LINE_HALO_LEVEL = 0.04;
static const float LINE_GAP = 6.0;       // lines stop short of the stars they join
static const float STAR_CORE = 1.35;
static const float STAR_HALO = 5.0;
static const float STAR_HALO_LEVEL = 0.05;
static const float STAR_LEVEL = 0.95;
static const float SPIKE_LEN = 30.0;
static const float SPIKE_W = 0.45;
static const float SPIKE_LEVEL = 0.22;
static const float TWINKLE = 0.16;
static const float PUPIL_SIZE = 2.1;
static const float PUPIL_HALO = 26.0;
static const float PUPIL_HALO_LEVEL = 0.05;
static const float BREATH_SECONDS = 19.0;

// The sky behind.
static const float SKY_CELL = 30.0;
static const float DUST_CELL = 11.0;
static const float SKY_SPIN_SECONDS = 2400.0;
static const float2 POLE = float2(0.35, -1.8); // in heights from the figure's centre
static const float SKY_SHARE = 0.32;
static const float SKY_LEVEL = 0.55;
static const float DUST_LEVEL = 0.05;
static const float SKY_HALO = 2.0;
static const float SKY_REACH = 6.0;      // a sky star's light ends inside its own cell

// The figure's stars: x, y in logo units and their size. The half-disc's rim (10-18) runs
// from the lid bar's east end round to its west; the south and east legs leave from it.
static const int NODE_COUNT = 19;
static const float3 NODES[19] = {
    float3(35, -69, 1.8),         // 0 north pad
    float3(14.4, -2.1, 0.5),      // 1 its leg's root, on the lid bar
    float3(-48, 6, 0.5),          // 2 west leg's root
    float3(-46, -46, 0.9),        // 3 west leg's bend
    float3(-145, -39, 1.25),      // 4 west ring
    float3(-53, 122, 1.8),        // 5 south pad
    float3(117, 54, 0.9),         // 6 east leg's bend
    float3(136, -6, 1.25),        // 7 east ring
    float3(-109, 15, 0.8),        // 8 lid bar, west end
    float3(95, -14, 0.8),         // 9 lid bar, east end
    float3(59.40, -8.45, 0.75),   // 10 rim at 0 degrees
    float3(57.41, 17.45, 0.8),    // 11
    float3(44.16, 40.61, 0.8),    // 12 the east leg's root
    float3(25.45, 54.34, 0.8),    // 13
    float3(1.15, 59.99, 0.8),     // 14
    float3(-25.26, 54.42, 0.8),   // 15 the south leg's root
    float3(-41.60, 43.23, 0.8),   // 16
    float3(-53.41, 27.33, 0.8),   // 17
    float3(-59.40, 8.45, 0.75)    // 18 rim at 180 degrees
};
static const int RING_W = 4;
static const int RING_E = 7;

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

float seg(float2 p, float2 a, float2 b)
{
    float2 pa = p - a;
    float2 ba = b - a;
    float h = saturate(dot(pa, ba) / dot(ba, ba));
    return length(pa - ba * h);
}

float2 rot(float2 p, float a)
{
    float c = cos(a);
    float s = sin(a);
    return float2(p.x * c + p.y * s, -p.x * s + p.y * c);
}

// A slow, smooth swell of a star's light; `x` counts its cycles.
float twinkle(float x)
{
    float f = frac(x);
    return 1.0 + TWINKLE * (8.0 * f * (1.0 - f) - 1.0);
}

// One chart line from star `a` to star `b`.
void chart(float2 s, float2 a, float2 b, inout float lineD)
{
    lineD = min(lineD, seg(s, a, b));
}

// A star's light at offset `d` (screen units): a soft core and a faint halo.
float starlight(float2 d, float size)
{
    float r2 = dot(d, d);
    float sig = STAR_CORE * size;
    float core = exp(-r2 / (2.0 * sig * sig));
    float k = STAR_HALO * size;
    return core + STAR_HALO_LEVEL * size / (1.0 + r2 / (k * k));
}

// Four fine rays, for the brightest stars.
float spikes(float2 d, float size)
{
    float2 a = abs(d);
    float len = SPIKE_LEN * size;
    float h = exp(-a.y / SPIKE_W) * saturate(1.0 - a.x / len);
    float v = exp(-a.x / SPIKE_W) * saturate(1.0 - a.y / len);
    return (h * h + v * v) * SPIKE_LEVEL;
}

// The sky's stars in one layer of cells; each one's light ends inside its cell.
float skylayer(float2 g, float cell, float share, float seed, float t, out float warmth)
{
    float2 id = floor(g / cell);
    float2 h = hash22(id + seed);
    float on = step(hash12(id + seed * 1.7), share);
    float2 d = g - (id + 0.2 + 0.6 * h) * cell;
    float r2 = dot(d, d);
    float mag = h.x * h.y;
    mag *= mag * mag;
    float sig = 0.7 + 1.0 * mag;
    float core = exp(-r2 / (2.0 * sig * sig));
    float halo = 0.12 / (1.0 + r2 / (SKY_HALO * SKY_HALO)) * saturate(1.0 - r2 / (SKY_REACH * SKY_REACH));
    float tw = twinkle(t * (0.06 + 0.08 * h.y) + h.x);
    warmth = h.y;
    return on * (core + halo) * (0.25 + 0.75 * mag) * tw;
}

float3 wall(float2 px)
{
    float H = resolution.y;
    float W = resolution.x;
    float pu = UNITS / H;                  // screen units per pixel
    float2 s = px * pu;
    float t = time * speed;
    float2 uv = px / H;

    float2 c = float2(0.5 * W / H + min(0.5 * W / H - RIGHT_SPACE, CENTRE_MAX), CENTRE_Y) * UNITS;

    // The sky: deep blue, a warm haze around the figure, a cool one across the top right.
    float3 col = lerp(SKY_HIGH, SKY_LOW, saturate(uv.y));
    float toMark = length((s - c) / UNITS * float2(0.7, 1.0));
    col += HAZE_WARM * exp(-toMark / HAZE_REACH);
    float band = (uv.y - 0.15) + 0.35 * (uv.x - c.x / UNITS);
    col += HAZE_COOL * exp(-band * band / 0.05) * smoothstep(0.2, 0.9, uv.x * H / W);

    // Its stars wheel slowly about a pole above the screen, fewer of them on the left.
    float spin = t * TAU / SKY_SPIN_SECONDS;
    float2 g = rot(s - (c + POLE * UNITS), spin);
    float leftCalm = smoothstep(0.02, 0.40, uv.x * H / W);
    float warmA;
    float warmB;
    float sky = skylayer(g, SKY_CELL, SKY_SHARE * stars * leftCalm, 3.0, t, warmA);
    float dust = skylayer(g, DUST_CELL, 0.5 * stars, 11.0, t, warmB);
    col += lerp(STAR_WHITE, STAR_GOLD, warmA * 0.6) * sky * SKY_LEVEL;
    col += STAR_WHITE * dust * DUST_LEVEL * leftCalm;

    // The figure is a plane turning slowly in depth: find where this pixel's ray meets it.
    float yaw = YAW_MAX * sway * sin(t * TAU / YAW_SECONDS);
    float pitch = PITCH_MAX * sway * sin(t * TAU / PITCH_SECONDS + 1.0);
    float cy = cos(yaw);
    float sy = sin(yaw);
    float cp = cos(pitch);
    float sp = sin(pitch);
    float3 ax = float3(cy, 0.0, -sy);
    float3 ay = float3(sp * sy, cp, sp * cy);
    float3 az = float3(cp * sy, -sp, cp * cy);
    float3 ray = float3((s - c) / MARK_SCALE, CAMERA);
    float3 hit = ray * (CAMERA * az.z / dot(ray, az)) - float3(0.0, 0.0, CAMERA);
    // In screen units at the figure's own scale.
    float2 f = float2(dot(hit, ax), dot(hit, ay)) * MARK_SCALE;

    float near = 1e5;
    float ringW = 0.0;
    float ringE = 0.0;
    float lit = 0.0;
    float rays = 0.0;
    [unroll] for (int i = 0; i < NODE_COUNT; i++)
    {
        float size = NODES[i].z;
        float tw = twinkle(t * (0.07 + 0.06 * hash12(float2(i, 9.0))) + i * 0.37);
        float2 d = f - NODES[i].xy * MARK_SCALE;
        float r = length(d);
        near = min(near, r);
        ringW = i == RING_W ? r : ringW;
        ringE = i == RING_E ? r : ringE;
        lit += starlight(d, size) * tw;
        if (size > 1.5)
        {
            rays += spikes(d, size) * tw;
        }
    }

    // The fine chart lines between them, stopping short of each star.
    float lineD = 1e5;
    chart(f, NODES[0].xy * MARK_SCALE, NODES[1].xy * MARK_SCALE, lineD);
    chart(f, NODES[2].xy * MARK_SCALE, NODES[3].xy * MARK_SCALE, lineD);
    chart(f, NODES[3].xy * MARK_SCALE, NODES[4].xy * MARK_SCALE, lineD);
    chart(f, NODES[15].xy * MARK_SCALE, NODES[5].xy * MARK_SCALE, lineD);
    chart(f, NODES[12].xy * MARK_SCALE, NODES[6].xy * MARK_SCALE, lineD);
    chart(f, NODES[6].xy * MARK_SCALE, NODES[7].xy * MARK_SCALE, lineD);
    chart(f, NODES[8].xy * MARK_SCALE, NODES[9].xy * MARK_SCALE, lineD);
    [unroll] for (int k = 10; k < NODE_COUNT - 1; k++)
    {
        chart(f, NODES[k].xy * MARK_SCALE, NODES[k + 1].xy * MARK_SCALE, lineD);
    }

    // Rings around the west and east stars.
    lineD += LINE_GAP * (1.0 - smoothstep(LINE_GAP * 0.5, LINE_GAP * 1.5, near));
    float ringR = RING_R * MARK_SCALE;
    float ringD = min(abs(ringW - ringR), abs(ringE - ringR));
    lineD = min(lineD, ringD + 0.15);

    float cover = saturate((LINE_HALF - lineD) / pu + 0.5);
    float halo = exp(-max(lineD - LINE_HALF, 0.0) / LINE_HALO) * LINE_HALO_LEVEL;
    col += LINE_GOLD * (cover * LINE_LEVEL + halo) * glow;
    col += STAR_GOLD * (lit * STAR_LEVEL + rays) * glow;

    // The pupil: the one star in Milva's own yellow, gliding slowly along the lid bar.
    float pupilX = 9.0 + sin(t * 0.05 + 1.3) * 20.0;
    float2 pd = f - rot(float2(pupilX, 7.0), -BAR_ANGLE) * MARK_SCALE;
    float breath = 0.85 + 0.15 * sin(time * TAU / BREATH_SECONDS);
    float pupil = starlight(pd, PUPIL_SIZE) + spikes(pd, PUPIL_SIZE) * 1.3;
    col += MILVA_YELLOW * pupil * glow * breath;
    col += MILVA_YELLOW * exp(-length(pd) / PUPIL_HALO) * PUPIL_HALO_LEVEL * glow * breath;

    col += (hash12(px) - 0.5) * DITHER;
    return col;
}
