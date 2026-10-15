// Milva beacon: the Milvasoft eye stands on the horizon of a dark, calm sea like a lighthouse.
// Its pupil is the lamp: a warm beam swings slowly out to one side, round behind the eye and
// out to the other, and the sea below mirrors the eye, the lamp and the beam in long broken
// glints. Sizes are shares of the screen's height, so it holds at any resolution and aspect
// ratio.

static const float TAU = 6.2831853;
static const float3 MILVA_YELLOW = float3(0.984, 0.769, 0.098);
static const float3 MILVA_ORANGE = float3(0.953, 0.518, 0.141);
static const float3 MILVA_GREY = float3(0.753, 0.749, 0.745);
static const float3 SKY_TOP = float3(0.008, 0.011, 0.022);
static const float3 SKY_LOW = float3(0.040, 0.044, 0.060);
static const float3 HORIZON_HAZE = float3(0.060, 0.052, 0.050);
static const float3 SEA_FAR = float3(0.020, 0.024, 0.034);
static const float3 SEA_NEAR = float3(0.006, 0.008, 0.013);
static const float3 BEAM_TINT = float3(1.00, 0.82, 0.50);
static const float3 STAR_TINT = float3(0.80, 0.85, 1.00);
static const float DITHER = 1.5 / 255.0;

// The eye, in logo units (crates/kabuk-scenes/src/milva.rs).
static const float BAR_ANGLE = -0.14137; // -8.1 degrees
static const float BODY_R = 60.0;
static const float PUPIL_R = 17.0;
static const float STROKE_HALF = 6.0;
static const float FOOT = 140.0;         // logo units from its centre to the south pad's foot

// Placement, in screen heights.
static const float HORIZON = 0.72;
static const float MARK_SCALE = 1.9 / 1080.0;
static const float RIGHT_SPACE = 0.62;   // from the right edge on 16:9 to 16:10
static const float CENTRE_MAX = 0.75;    // at most this far right of the middle

// The eye in the night: cool grey, warmed near its lamp.
static const float MARK_LEVEL = 0.13;
static const float3 MARK_COOL = float3(0.92, 0.96, 1.06);
static const float RIM_LEVEL = 0.10;
static const float RIM_WIDTH = 1.4;       // logo units
static const float MARK_WARM = 0.08;
static const float MARK_WARM_REACH = 70.0; // logo units
static const float LAMP_LEVEL = 0.95;
static const float LAMP_CORE = 0.008;
static const float LAMP_HALO = 0.07;
static const float LAMP_HALO_LEVEL = 0.10;

// The beam: it swings round behind the eye, never towards the one looking at it.
static const float SWING = 1.9;          // radians either side of straight away
static const float SWING_SECONDS = 96.0;
static const float BEAM_DIP = 0.035;     // it falls a little towards the sea
static const float BEAM_LEN = 1.3;
static const float BEAM_MIN = 0.06;
static const float BEAM_W0 = 0.006;
static const float BEAM_SPREAD = 0.10;
static const float BEAM_LEVEL = 0.20;
static const float HAZE_LEVEL = 0.025;

// The sea.
static const float REFLECT = 0.55;
static const float WAVE_SHIFT = 0.010;   // how far a wave moves a reflection, in heights
static const float GLINT_POWER = 3.0;
static const float PATH_LEVEL = 0.22;
static const float PATH_WIDTH = 0.004;   // in heights at the horizon
static const float PATH_WIDEN = 0.30;    // how fast it widens towards us
static const float PATH_FADE = 2.5;
static const float SPARKLE_POWER = 6.0;
static const float TOWARDS_GLOW = 1.5;   // how much brighter the lamp looks as the beam turns our way
static const float STAR_CELL = 0.028;
static const float STAR_SHARE = 0.18;
static const float STAR_LEVEL = 0.30;

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
    float horizon = HORIZON * H;
    float S = H * MARK_SCALE;              // pixels per logo unit
    float2 c = float2(0.5 * W + min(0.5 * W - RIGHT_SPACE * H, CENTRE_MAX * H), horizon - FOOT * S);
    float eu = 1.0 / S;

    float pupilX = 12.0 + sin(t * 0.05 + 1.3) * 12.0;
    float2 lampE = rot(float2(pupilX, 7.0), -BAR_ANGLE);
    float2 lamp = c + lampE * S;

    // On the sea, look up the sky's reflection instead: mirrored at the horizon, shifted by
    // the waves, which are finer and slower towards the horizon.
    bool sea = px.y > horizon;
    float k = max(px.y - horizon, 0.0) / H;          // depth below the horizon
    float wz = 1.0 / (k + 0.006);                     // distance across the water
    float wx = (px.x - lamp.x) / H * wz;
    float swell = sin(wz * 1.7 - t * 0.55 + sin(wx * 0.31 + t * 0.12) * 2.2);
    float chop = sin(wz * 3.1 + wx * 0.23 + t * 0.71 + swell);
    float shift = WAVE_SHIFT * waves * H * (k * 4.0 + 0.15);
    float2 p = sea ? float2(px.x + shift * swell, 2.0 * horizon - px.y + shift * chop * 0.6) : px;

    // The sky.
    float2 uv = p / H;
    float up = saturate(1.0 - uv.y / HORIZON);
    float3 col = lerp(SKY_LOW, SKY_TOP, sqrt(up));
    col += HORIZON_HAZE * exp(-up * 9.0);
    float2 cell = floor(uv / STAR_CELL);
    float2 at = (cell + 0.25 + 0.5 * float2(hash12(cell), hash12(cell + 7.1))) * STAR_CELL;
    float star = step(hash12(cell + 3.3), STAR_SHARE) * smoothstep(0.15, 0.6, up) * smoothstep(0.1, 0.45, p.x / W);
    col += STAR_TINT * star * exp(-dot(uv - at, uv - at) * H * H / 1.2) * STAR_LEVEL * hash12(cell + 5.9);

    // The beam swings about straight away from us; its apparent length follows its side-on
    // share, and it fades towards the icons on the left.
    float phi = SWING * sin(t * TAU / SWING_SECONDS);
    float side = sin(phi);
    float2 dir = normalize(float2(side >= 0.0 ? 1.0 : -1.0, BEAM_DIP));
    float2 rel = p - lamp;
    float along = dot(rel, dir);
    float across = abs(dot(rel, float2(-dir.y, dir.x)));
    float width = BEAM_W0 * H + max(along, 0.0) * BEAM_SPREAD;
    float reach = (BEAM_LEN * abs(side) + BEAM_MIN) * H;
    float shaft = exp(-across * across / (width * width)) * saturate(along / (0.02 * H)) * exp(-max(along, 0.0) / reach);
    shaft *= smoothstep(0.05, 0.40, p.x / W) * smoothstep(0.08, 0.45, abs(side));
    float r = length(rel) / H;
    float towards = saturate(-cos(phi));
    float haze = exp(-r / LAMP_HALO) * LAMP_HALO_LEVEL * (1.0 + towards * TOWARDS_GLOW) + exp(-r / 0.25) * HAZE_LEVEL * (0.6 + 0.4 * abs(side));

    // The eye: grey in the night, warmed around its lamp, the lamp itself in Milva yellow.
    float2 e = (p - c) / S;
    float pupil;
    float d = mark(e, pupilX, pupil);
    float inside = saturate(0.5 - d / eu);
    float3 eye = MILVA_GREY * MARK_COOL * MARK_LEVEL * (0.75 + 0.25 * saturate(-e.y / 120.0 + 0.5));
    eye += MILVA_GREY * MARK_COOL * RIM_LEVEL * exp(min(d, 0.0) / RIM_WIDTH);
    eye += MILVA_ORANGE * MARK_WARM * exp(-length(e - lampE) / MARK_WARM_REACH) * glow;
    col = lerp(col, eye, inside);
    float lit = saturate(0.5 - pupil / eu);
    float2 pq = rot(e, BAR_ANGLE) - float2(pupilX, 0.0);
    float core = saturate(1.0 - length(pq - float2(0.0, 5.0)) / PUPIL_R);
    col = lerp(col, lerp(MILVA_ORANGE, MILVA_YELLOW, core) * LAMP_LEVEL * glow, lit);
    col += MILVA_YELLOW * exp(-r / LAMP_CORE) * 0.5 * glow;
    col += BEAM_TINT * (shaft * BEAM_LEVEL * beam * (1.0 - 0.6 * inside) + haze) * glow;

    // The sea: dark water, the reflection broken into glints along the wave crests.
    if (sea)
    {
        float glint = pow(0.5 + 0.5 * chop * swell, GLINT_POWER);
        float3 water = lerp(SEA_FAR, SEA_NEAR, saturate(k * 4.0));
        col = water + col * REFLECT * (0.25 + 0.75 * glint);
        // The lamp's own path across the water, from the horizon towards us, in short glints.
        float pathW = (PATH_WIDTH + k * PATH_WIDEN) * H;
        float dx = (px.x - lamp.x - shift * swell * 2.0) / pathW;
        float sparkle = pow(saturate(0.5 + 0.5 * sin(wz * 5.3 + wx * 1.7 + t * 0.9 + chop * 2.0)), SPARKLE_POWER);
        float path = exp(-dx * dx) * exp(-k * PATH_FADE) * (0.3 + 0.7 * sparkle);
        col += lerp(MILVA_ORANGE, MILVA_YELLOW, 0.6) * path * PATH_LEVEL * glow * (1.0 + towards);
    }

    col += (hash12(px) - 0.5) * DITHER;
    return col;
}
