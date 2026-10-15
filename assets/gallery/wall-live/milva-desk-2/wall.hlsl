// Milva relief: the Milvasoft eye pressed into a dark graphite surface. A warm light circles
// it very slowly, so the bevels on the side facing it catch a thin golden edge while the far
// side falls into shadow, and the soft sheen on the surface turns with it. Only the pupil
// holds colour of its own: a recessed yellow light that breathes. Sizes are shares of the
// screen's height, so it holds at any resolution and aspect ratio.

static const float3 MILVA_YELLOW = float3(0.984, 0.769, 0.098);
static const float3 MILVA_ORANGE = float3(0.953, 0.518, 0.141);
static const float3 MILVA_GREY = float3(0.753, 0.749, 0.745);
static const float3 SURFACE = float3(0.046, 0.046, 0.050);
static const float3 SURFACE_FAR = float3(0.030, 0.030, 0.034);
static const float3 LIGHT_TINT = float3(1.00, 0.86, 0.62);
static const float DITHER = 1.5 / 255.0;

// The eye, in logo units (crates/kabuk-scenes/src/milva.rs).
static const float BAR_ANGLE = -0.14137; // -8.1 degrees
static const float BODY_R = 60.0;
static const float PUPIL_R = 17.0;
static const float STROKE_HALF = 6.0;

// Placement: logo units per screen height, and where its centre sits (in screen heights).
static const float MARK_SCALE = 2.4 / 1080.0;
static const float RIGHT_SPACE = 0.62;   // from the right edge on 16:9 to 16:10
static const float CENTRE_MAX = 0.75;    // at most this far right of the middle
static const float CENTRE_Y = 0.46;

// Light and relief.
static const float ORBIT_SECONDS = 150.0;
static const float BEVEL = 2.0;          // logo units of bevel inside each edge
static const float EDGE_LEVEL = 0.62;
static const float SHADE_LEVEL = 0.030;
static const float CREST_W = 0.45;       // logo units
static const float CREST_LEVEL = 0.30;
static const float SHADOW_OFFSET = 4.5;  // logo units the cast shadow falls away from the light
static const float SHADOW_SOFT = 4.0;
static const float SHADOW_LEVEL = 0.6;
static const float SHEEN_LEVEL = 0.03;
static const float SHEEN_REACH = 1.1;    // in screen heights
static const float SHEEN_ORBIT = 0.55;   // how far the sheen's centre circles, in heights
static const float PUPIL_LEVEL = 0.6;
static const float PUPIL_GLOW = 0.05;
static const float BREATH_SECONDS = 23.0;

float seg(float2 p, float2 a, float2 b)
{
    float2 pa = p - a;
    float2 ba = b - a;
    float h = saturate(dot(pa, ba) / dot(ba, ba));
    return length(pa - ba * h);
}

float hash12(float2 p)
{
    float3 p3 = frac(float3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
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
    float S = H * MARK_SCALE;              // pixels per logo unit
    float2 e = (px - c) / S;
    float eu = 1.0 / S;                    // logo units per pixel

    // The light circles the mark; L points from the mark towards it.
    float a = t * 6.2832 / ORBIT_SECONDS + 2.2;
    float2 L = float2(cos(a), sin(a));

    float pupilX = 9.0 + sin(t * 0.07 + 1.3) * 20.0;
    float pupil;
    float unused;
    float d = mark(e, pupilX, pupil);
    float dl = mark(e + L * BEVEL, pupilX, unused);
    float ds = mark(e + L * SHADOW_OFFSET, pupilX, unused);

    // The surface, with a broad sheen that turns with the light.
    float2 uv = (px - c) / H;
    float sheen = exp(-length(uv - L * SHEEN_ORBIT) / SHEEN_REACH);
    float3 col = lerp(SURFACE_FAR, SURFACE, saturate(1.2 - length(uv) * 0.8));
    col += LIGHT_TINT * SHEEN_LEVEL * sheen * glow;

    // Relief: the mark is raised; its edges facing the light catch it, the far ones darken.
    float inside = saturate(0.5 - d / eu);
    float band = saturate(1.0 + d / BEVEL) * inside;          // 1 at the edge, 0 deeper in
    float slope = clamp((dl - d) / BEVEL, -1.0, 1.0);       // > 0 where the edge faces the light
    float lit = saturate(slope) * band;
    float dark = saturate(-slope) * band;
    col += lerp(MILVA_ORANGE, LIGHT_TINT, lit) * pow(lit, 1.5) * EDGE_LEVEL * glow;
    // A fine crest right on the edge, where the light grazes it.
    float crest = exp(-max(-d, 0.0) / CREST_W) * inside * saturate(slope);
    col += MILVA_YELLOW * crest * crest * CREST_LEVEL * glow;
    col *= 1.0 - dark * 0.55;
    col += LIGHT_TINT * SHADE_LEVEL * inside * (0.5 + 0.5 * sheen);

    // Its shadow falls away from the light.
    float shadow = (1.0 - inside) * smoothstep(SHADOW_SOFT, -SHADOW_SOFT, ds);
    col *= 1.0 - shadow * SHADOW_LEVEL;

    // The pupil is a hollow lit from within.
    float breath = 0.82 + 0.18 * sin(time * 6.2832 / BREATH_SECONDS);
    float pin = saturate(0.5 - pupil / eu);
    float2 q = rot(e, BAR_ANGLE) - float2(pupilX, 0.0);
    float core = saturate(1.0 - length(q) / PUPIL_R);
    float rim = saturate(1.0 + pupil / BEVEL);
    float3 light = lerp(MILVA_ORANGE, MILVA_YELLOW, core) * (0.45 + 0.55 * core);
    col = lerp(col, light * PUPIL_LEVEL * glow * breath * (1.0 - 0.4 * rim), pin);
    float2 pupilPx = c + rot(float2(pupilX, 4.0), -BAR_ANGLE) * S;
    float r = length(px - pupilPx) / H;
    col += MILVA_ORANGE * PUPIL_GLOW * glow * breath * exp(-r / 0.10) * (1.0 - inside);

    col += (hash12(px) - 0.5) * DITHER;
    return col;
}
