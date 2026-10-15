// Milva liquid gold: the Milvasoft eye cast in molten gold, resting in dark water. Its
// rounded strokes catch a soft strip of light that sweeps slowly across them while the metal's
// surface wavers as if still liquid; the pupil is a pool of Milva yellow lit from within. Fine
// gold outlines of the mark leave it like rings of ink in still water, drifting outward,
// bending and fading. Sizes are in logo units scaled to the screen's height, so it holds at
// any resolution and aspect ratio.

static const float TAU = 6.2831853;
static const float3 MILVA_YELLOW = float3(0.984, 0.769, 0.098);
static const float3 MILVA_ORANGE = float3(0.953, 0.518, 0.141);
static const float3 GOLD = float3(1.00, 0.76, 0.34);
static const float3 GOLD_DEEP = float3(0.30, 0.16, 0.04);
static const float3 WATER = float3(0.020, 0.017, 0.014);
static const float3 WATER_FAR = float3(0.010, 0.010, 0.011);
static const float3 WATER_WARM = float3(0.040, 0.026, 0.010);
static const float DITHER = 1.5 / 255.0;

// The eye, in logo units (crates/kabuk-scenes/src/milva.rs).
static const float BAR_ANGLE = -0.14137; // -8.1 degrees
static const float BODY_R = 60.0;
static const float PUPIL_R = 17.0;
static const float STROKE_HALF = 6.0;

// Placement: logo units per 1080 lines, and where its centre sits (in screen heights).
static const float MARK_SCALE = 2.2;
static const float RIGHT_SPACE = 0.62;   // from the right edge on 16:9 to 16:10
static const float CENTRE_MAX = 0.75;    // at most this far right of the middle
static const float CENTRE_Y = 0.46;

// The metal.
static const float BEVEL = 6.0;          // logo units of rounded edge
static const float DOME = 1.0 / 260.0;   // the whole mark bulges gently, so light travels over it
static const float WAVER = 0.10;         // how far the liquid surface tilts
static const float SWEEP_SECONDS = 70.0;
static const float TURN_SECONDS = 230.0;
static const float STRIP_WIDTH = 0.10;
static const float STRIP_LEVEL = 0.85;
static const float KEY_LEVEL = 0.30;
static const float AMBIENT = 0.10;
static const float PUPIL_LEVEL = 0.95;
static const float PUPIL_GLOW = 0.06;
static const float BREATH_SECONDS = 23.0;

// The ink rings in the water.
static const float RING_SPACING = 30.0;  // logo units between rings
static const float RING_HALF = 0.7;
static const float RING_SPREAD = 0.025;  // how much wider a ring grows per logo unit it travels
static const float RING_DRIFT = 0.045;   // rings per second outward
static const float RING_REACH = 80.0;
static const float RING_LEVEL = 0.30;
static const float WARP_START = 70.0;    // logo units from the mark where the water bends them fully
static const float WARP = 24.0;
static const float HAZE_LEVEL = 0.05;
static const float HAZE_REACH = 40.0;

float hash12(float2 p)
{
    float3 p3 = frac(float3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
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

// The mark's signed distance in logo units; `pupil` gets the pupil's own distance.
float mark(float2 e, float pupilX, out float pupil)
{
    float ink = seg(e, float2(13, -12), float2(35, -69));
    ink = min(ink, seg(e, float2(-48, 6), float2(-46, -46)));
    ink = min(ink, seg(e, float2(-46, -46), float2(-130, -38)));
    ink = min(ink, seg(e, float2(-25, 54), float2(-53, 122)));
    ink = min(ink, seg(e, float2(50, 46), float2(117, 54)));
    ink = min(ink, seg(e, float2(117, 54), float2(132, 10)));
    ink = min(ink, seg(e, float2(-109, 15), float2(95, -14)));
    ink -= STROKE_HALF;
    float2 q = rot(e, BAR_ANGLE);
    float body = max(length(q) - BODY_R, -q.y);
    pupil = max(length(q - float2(pupilX, 0.0)) - PUPIL_R, -q.y);
    body = max(body, -pupil);
    ink = min(ink, body);
    float pads = min(length(e - float2(35, -69)) - 18.0, length(e - float2(-53, 122)) - 18.0);
    pads = min(pads, abs(length(e - float2(-145, -39)) - 11.0) - 4.0);
    pads = min(pads, abs(length(e - float2(136, -6)) - 11.0) - 4.0);
    return min(ink, pads);
}

float3 wall(float2 px)
{
    float H = resolution.y;
    float W = resolution.x;
    float t = time * speed;

    float2 c = float2(0.5 * W + min(0.5 * W - RIGHT_SPACE * H, CENTRE_MAX * H), CENTRE_Y * H);
    float S = H * MARK_SCALE / 1080.0;     // pixels per logo unit
    float2 e = (px - c) / S;
    float eu = 1.0 / S;                    // logo units per pixel

    float pupilX = 9.0 + sin(t * 0.06 + 1.3) * 20.0;
    float pupil;
    float unused;
    float d = mark(e, pupilX, pupil);

    // The water: warm near the gold, darker towards the edges.
    float2 uv = (px - c) / H;
    float3 col = lerp(WATER_FAR, WATER, saturate(1.15 - length(uv) * 0.9));
    col += WATER_WARM * exp(-length(uv) / 0.35);

    // Ink rings: the mark's outline repeated outward through water that bends it more the
    // further it goes; each ring drifts slowly out and fades.
    float far = smoothstep(0.0, WARP_START, d) * WARP * flow;
    float2 q = e;
    q += far * float2(sin(q.y * 0.031 + t * 0.11), sin(q.x * 0.027 - t * 0.09));
    q += far * 0.6 * float2(sin(q.y * 0.053 - t * 0.07 + 1.7), sin(q.x * 0.047 + t * 0.08 + 0.4));
    float dw = max(mark(q, pupilX, unused), 0.0);
    float phase = dw / RING_SPACING - t * RING_DRIFT;
    float ringD = abs(frac(phase) - 0.5) * RING_SPACING;
    float ringHalf = RING_HALF + dw * RING_SPREAD;
    float ringCover = saturate((ringHalf - ringD) / (eu + ringHalf * 0.6) + 0.5) * RING_HALF / ringHalf;
    float strand = 0.55 + 0.45 * sin(q.x * 0.035 + q.y * 0.022 + t * 0.07 + floor(phase) * 2.1);
    float open = saturate(pupil / eu);
    float reach = exp(-dw / RING_REACH) * smoothstep(4.0, 18.0, dw) * open;
    col += GOLD * ringCover * strand * RING_LEVEL * ripples * reach * glow;
    col += GOLD * exp(-dw / HAZE_REACH) * HAZE_LEVEL * open * glow;

    // The metal: rounded edges over a gentle bulge, its liquid surface wavering.
    float hstep = 0.75;
    float2 g = float2(mark(e + float2(hstep, 0.0), pupilX, unused), mark(e + float2(0.0, hstep), pupilX, unused)) - d;
    g /= max(length(g), 1e-4);
    float u = 1.0 - saturate(-d / BEVEL);
    float3 n = float3(g * u, sqrt(saturate(1.0 - u * u)));
    n.xy += e * DOME;
    n.xy += WAVER * flow * float2(sin(e.x * 0.09 + e.y * 0.05 + t * 0.30), sin(e.y * 0.08 - e.x * 0.04 - t * 0.25));
    n = normalize(n);
    float3 r = float3(2.0 * n.z * n.xy, 2.0 * n.z * n.z - 1.0);

    // A long soft strip light sweeps slowly across, turning as it goes; a warm key above left.
    float turn = t * TAU / TURN_SECONDS;
    float2 across = float2(cos(turn), sin(turn));
    float at = sin(t * TAU / SWEEP_SECONDS) * 0.75;
    float strip = dot(r.xy, across) - at;
    strip = exp(-strip * strip / STRIP_WIDTH);
    float key = saturate(dot(r, normalize(float3(-0.45, -0.55, 0.70))));
    key *= key;
    key *= key;
    float3 metal = GOLD_DEEP * (0.4 + 0.6 * n.z) + GOLD * (AMBIENT + strip * STRIP_LEVEL + key * KEY_LEVEL) * glow;
    float inside = saturate(0.5 - d / eu);
    col = lerp(col, metal, inside);

    // The pupil: a pool of molten yellow, brightest at its heart, breathing slowly.
    float breath = 0.85 + 0.15 * sin(time * TAU / BREATH_SECONDS);
    float2 pq = rot(e, BAR_ANGLE) - float2(pupilX, 0.0);
    float core = saturate(1.0 - length(pq - float2(0.0, 5.0)) / PUPIL_R);
    float swirl = 0.9 + 0.1 * sin(pq.x * 0.4 + pq.y * 0.3 + t * 0.5);
    float3 molten = lerp(MILVA_ORANGE, MILVA_YELLOW, core) * (0.7 + 0.3 * core) * swirl;
    float pin = saturate(0.5 - pupil / eu);
    col = lerp(col, molten * PUPIL_LEVEL * glow * breath, pin);
    float2 pupilPx = c + rot(float2(pupilX, 6.0), -BAR_ANGLE) * S;
    col += MILVA_ORANGE * PUPIL_GLOW * glow * breath * exp(-length(px - pupilPx) / (0.08 * H)) * (1.0 - inside);

    col += (hash12(px) - 0.5) * DITHER;
    return col;
}
