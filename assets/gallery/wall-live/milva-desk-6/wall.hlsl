// Milva Bauhaus: a flat poster in Milva yellow, orange, paper and graphite. A large yellow
// disc sits behind the Milvasoft eye; where the eye crosses a coloured shape it turns to
// graphite, elsewhere it is paper, and its yellow pupil reads as a hole cut through to the
// disc. An orange half-disc turns very slowly, the disc drifts a little, a small square
// travels along a ruled line. A fine print grain keeps the flat colours from looking digital.
// Sizes are shares of the screen's height, so it holds at any resolution and aspect ratio.

static const float TAU = 6.2831853;
static const float3 MILVA_YELLOW = float3(0.984, 0.769, 0.098);
static const float3 MILVA_ORANGE = float3(0.953, 0.518, 0.141);
static const float3 GRAPHITE = float3(0.105, 0.105, 0.112);
static const float3 GRAPHITE_DEEP = float3(0.062, 0.062, 0.068);
static const float3 PAPER = float3(0.86, 0.84, 0.79);
static const float3 MARK_ON_COLOUR = float3(0.115, 0.112, 0.112);
static const float YELLOW_LEVEL = 0.90;
static const float ORANGE_LEVEL = 0.82;
static const float PAPER_LINE_LEVEL = 0.55;
static const float GRAIN_LEVEL = 0.018;

// The eye, in logo units (crates/kabuk-scenes/src/milva.rs).
static const float BAR_ANGLE = -0.14137; // -8.1 degrees
static const float BODY_R = 60.0;
static const float PUPIL_R = 17.0;
static const float STROKE_HALF = 6.0;

// Placement: the composition's centre, in screen heights.
static const float MARK_SCALE = 2.0 / 1080.0;
static const float RIGHT_SPACE = 0.62;   // from the right edge on 16:9 to 16:10
static const float CENTRE_MAX = 0.75;    // at most this far right of the middle
static const float CENTRE_Y = 0.46;

// The shapes, in screen heights from the composition's centre.
static const float2 DISC_AT = float2(0.05, -0.03);
static const float DISC_R = 0.30;
static const float DISC_DRIFT = 0.022;
static const float DISC_SECONDS = 260.0;
static const float BAND_FROM = 0.42;
static const float2 HALF_AT = float2(-0.34, 0.27);
static const float HALF_R = 0.13;
static const float HALF_SECONDS = 340.0;
static const float RULE_Y = 0.37;
static const float RULE_FROM = -0.62;
static const float RULE_HALF = 0.0014;
static const float SQUARE_HALF = 0.011;
static const float SQUARE_SECONDS = 180.0;
static const float SQUARE_TRAVEL = 0.34;
static const float2 RING_AT = float2(0.30, -0.32);
static const float RING_R = 0.045;
static const float RING_HALF = 0.0035;
static const float2 DOTS_AT = float2(-0.56, -0.34);
static const float DOT_STEP = 0.030;
static const float DOT_R = 0.0042;
static const float PUPIL_SECONDS = 120.0;

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

// How much of this pixel a shape at signed distance `d` (in heights) covers.
float cover(float d, float pw)
{
    return saturate(0.5 - d / pw);
}

float3 wall(float2 px)
{
    float H = resolution.y;
    float W = resolution.x;
    float t = time * speed;
    float pw = 1.0 / H;                    // one pixel, in heights

    float2 c = float2(0.5 * W / H + min(0.5 * W / H - RIGHT_SPACE, CENTRE_MAX), CENTRE_Y);
    float2 p = px / H - c;

    float3 col = GRAPHITE;

    // A deep graphite band out to the right edge.
    float band = cover(BAND_FROM - p.x, pw);
    col = lerp(col, GRAPHITE_DEEP, band);

    // The yellow disc, drifting on a small slow circle.
    float da = t * TAU / DISC_SECONDS;
    float2 discC = DISC_AT + DISC_DRIFT * drift * float2(cos(da), sin(da));
    float disc = cover(length(p - discC) - DISC_R, pw);
    col = lerp(col, MILVA_YELLOW * YELLOW_LEVEL, disc);

    // The orange half-disc, turning very slowly about its centre.
    float ha = t * TAU / HALF_SECONDS * drift + 0.6;
    float2 hp = p - HALF_AT;
    float halfDisc = cover(max(length(hp) - HALF_R, dot(hp, float2(cos(ha), sin(ha)))), pw);
    col = lerp(col, MILVA_ORANGE * ORANGE_LEVEL, halfDisc);

    // A ruled line with a small square travelling along it.
    float rule = cover(max(abs(p.y - RULE_Y) - RULE_HALF, RULE_FROM - p.x), pw);
    col = lerp(col, PAPER * PAPER_LINE_LEVEL, rule);
    float sx = SQUARE_TRAVEL * sin(t * TAU / SQUARE_SECONDS) * drift;
    float2 sq = abs(p - float2(sx, RULE_Y)) - SQUARE_HALF;
    float square = cover(max(sq.x, sq.y), pw);
    col = lerp(col, PAPER, square);

    // A thin ring and a small grid of dots.
    float ring = cover(abs(length(p - RING_AT) - RING_R) - RING_HALF, pw);
    col = lerp(col, PAPER * PAPER_LINE_LEVEL, ring);
    float2 g = p - DOTS_AT;
    float2 cell = clamp(round(g / DOT_STEP), 0.0, 3.0);
    float dots = cover(length(g - cell * DOT_STEP) - DOT_R, pw);
    col = lerp(col, PAPER * PAPER_LINE_LEVEL, dots);

    // The eye: paper on graphite, graphite where it crosses a colour; its pupil is yellow.
    float S = MARK_SCALE;                  // heights per logo unit
    float pupilX = 9.0 + sin(t * TAU / PUPIL_SECONDS) * 20.0;
    float pupil;
    float d = mark(p / S, pupilX, pupil);
    float eye = cover(d * S, pw);
    float3 ink = lerp(PAPER, MARK_ON_COLOUR, saturate(disc + halfDisc));
    col = lerp(col, ink, eye);
    col = lerp(col, MILVA_YELLOW * YELLOW_LEVEL, cover(pupil * S, pw));

    // Print grain, fixed in place.
    col *= 1.0 + (hash12(px) - 0.5) * 2.0 * GRAIN_LEVEL * grain;
    col += (hash12(px + 17.0) - 0.5) / 255.0;
    return col;
}
