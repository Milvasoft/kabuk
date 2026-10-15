// Milva circuit board: the Milvasoft eye rests to the right of centre on a quiet board of
// 45-degree trace buses; now and then a signal leaves one of its pads and travels slowly out
// along a trace. Everything is laid out in "board units" (pixels of a 1080-line screen), so
// it looks the same at any resolution and aspect ratio.

static const float3 MILVA_YELLOW = float3(0.984, 0.769, 0.098);
static const float3 MILVA_ORANGE = float3(0.953, 0.518, 0.141);
static const float3 MILVA_GREY = float3(0.753, 0.749, 0.745);
static const float3 DESK = float3(0.040, 0.040, 0.046);
static const float3 DESK_WARM = float3(0.030, 0.022, 0.010);

// The board: buses of parallel traces that jog together at 45 degrees.
static const float LANE = 15.0;          // spacing of neighbouring traces
static const float LANES = 5.0;          // traces per bus
static const float SWING = 3.0;          // lanes a bus moves by when it jogs
static const float BAND = 165.0;         // height of one bus's band (11 lanes)
static const float RUN = 135.0;          // length of one bus segment (9 lanes)
static const float TRACE_HALF = 1.5;     // half the width of a board trace
static const float VIA_R = 4.2;          // board via radius
static const float BOARD_LEVEL = 0.09;
static const float SIGNAL_PERIOD = 2600.0;
static const float SIGNAL_TRAIL = 70.0;
static const float SIGNAL_SPEED = 38.0;  // board units per second at speed 1
static const float SIGNAL_FRONT = 40.0;  // the soft leading edge, so nothing pops on
static const float EYE_PERIOD = 1500.0;
static const float PAD_GLOW = 0.62;

// The eye, in logo units (crates/kabuk-scenes/src/milva.rs).
static const float EYE_SCALE = 1.25;
static const float DITHER = 1.5 / 255.0;
static const float BAR_ANGLE = -0.14137; // -8.1 degrees
static const float BODY_R = 60.0;
static const float PUPIL_R = 17.0;
static const float STROKE_HALF = 6.0;
static const float EYE_LEVEL = 0.30;
static const float EYE_TRACE_LEVEL = 0.12;
static const float EYE_TRACE_HALF = 2.4;
static const float BLINK_EVERY = 29.0;
static const float BLINK_SECONDS = 0.55;

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

// Distance to a segment, keeping the nearest one and how far along the path it is.
void seg_along(float2 p, float2 a, float2 b, float start, inout float best, inout float along)
{
    float2 pa = p - a;
    float2 ba = b - a;
    float len = length(ba);
    float h = saturate(dot(pa, ba) / (len * len));
    float d = length(pa - ba * h);
    if (d < best)
    {
        best = d;
        along = start + h * len;
    }
}

// A slow signal: light at `s` along a path whose signals start at `phase`; 0 when dark.
float signal(float s, float t, float phase, float period, float seed)
{
    float x = (t * SIGNAL_SPEED * speed - s) / period + phase;
    float cycle = floor(x);
    float q = (x - cycle) * period;                 // distance behind the head
    float on = step(hash12(float2(seed, cycle)), 0.38);
    float head = exp(-q / SIGNAL_TRAIL) * smoothstep(period, period - SIGNAL_FRONT, q);
    return head * on;
}

// x: distance to the nearest board trace, y: its signal, z: distance to a via's ink.
float3 buses(float2 b, float t)
{
    float band = floor(b.y / BAND);
    float hb = hash12(float2(band, 7.0));
    float ly = b.y - band * BAND - 1.5 * LANE;
    float xr = b.x + hb * RUN * 13.0;
    float k = floor(xr / RUN);
    float xs = xr - k * RUN;
    float o0 = SWING * LANE * step(0.5, hash12(float2(band, k - 1.0)));
    float o1 = SWING * LANE * step(0.5, hash12(float2(band, k)));
    float jog = o1 - o0;
    float up = jog < 0.0 ? 1.0 : 0.0;
    float dir = hb < 0.5 ? 1.0 : -1.0;
    // Buses carry three to five traces; some bands stay empty.
    float hn = hash12(float2(band, 19.0));
    float lanes = hn < 0.1 ? 0.0 : 3.0 + floor(hn * 2.99);
    // Find the nearest trace of the bus by its shape alone.
    float ramp = abs(jog);
    float best = 1e5;
    float diag = 1.0;
    float pick = 0.0;
    [unroll] for (int i = 0; i < 5; i++)
    {
        float fi = i;
        // Nested bends keep the spacing even along the 45-degree part.
        float x0 = 2.5 * LANE + lerp(LANES - 1.0 - fi, fi, up) * 0.41421 * LANE;
        float r = clamp(xs - x0, 0.0, ramp);
        float y = fi * LANE + o0 + r * sign(jog);
        float dg = (xs > x0 && xs < x0 + ramp) ? 0.70711 : 1.0;
        float d = abs(ly - y) * dg;
        if (d < best)
        {
            best = d;
            diag = dg;
            pick = fi;
        }
    }
    // Then whether that trace is there, where it ends and what it carries.
    float lane = band * 8.0 + pick;
    float on = step(0.26, hash12(float2(lane, k))) * step(pick + 0.5, lanes);
    float onPrev = step(0.26, hash12(float2(lane, k - 1.0)));
    float onNext = step(0.26, hash12(float2(lane, k + 1.0)));
    float a = onPrev > 0.5 ? -LANE : 1.2 * LANE;
    float e = onNext > 0.5 ? RUN + LANE : RUN - 1.0 * LANE;
    float xc = clamp(xs, a, e);
    float dist = on > 0.5 ? (xs == xc ? best : length(float2(xs - xc, best))) : 1e5;
    float light = signal(xr * dir, t, hash12(float2(lane, 3.0)), SIGNAL_PERIOD, lane);
    float ring = step(0.5, hash12(float2(lane, k + 50.0)));
    float va = length(float2(xs - a, ly - (pick * LANE + o0)));
    float ve = length(float2(xs - e, ly - (pick * LANE + o1)));
    float vd = min(onPrev > 0.5 ? 1e5 : va, onNext > 0.5 ? 1e5 : ve);
    vd = ring > 0.5 ? abs(vd - VIA_R) - 1.2 : vd - VIA_R;
    float via = on > 0.5 ? vd : 1e5;
    return float3(dist, light, via);
}

float2 rot(float2 p, float a)
{
    float c = cos(a);
    float s = sin(a);
    return float2(p.x * c + p.y * s, -p.x * s + p.y * c);
}

float3 wall(float2 px)
{
    float H = resolution.y;
    float W = resolution.x;
    float unit = 1080.0 / H;               // board units per pixel
    float2 b = px * unit;
    float t = time;

    // The eye sits right of centre; on very wide screens it keeps nearer the middle.
    float cxH = 0.5 * W / H + min(0.5 * W / H - 0.62, 0.75);
    float2 eyeC = float2(cxH * 1080.0, 0.42 * 1080.0);
    float2 e = (b - eyeC) / EYE_SCALE;     // logo units
    float eu = unit / EYE_SCALE;           // logo units per pixel

    float3 col = DESK;
    float toEye = length((b - eyeC) * float2(0.8, 1.0));
    col += DESK_WARM * exp(-toEye / 520.0);

    // ---- the eye's own traces, leaving its four pads
    float tb = 1e5;
    float ta = 0.0;
    float tid = 0.0;
    float best = 1e5;
    float along = 0.0;
    // North via, up to the top edge.
    seg_along(e, float2(35, -69), float2(35, -200), 0.0, best, along);
    seg_along(e, float2(35, -200), float2(115, -280), 131.0, best, along);
    seg_along(e, float2(115, -280), float2(115, -1400), 244.1, best, along);
    if (best < tb) { tb = best; ta = along; tid = 1.0; }
    best = 1e5;
    // West ring, a short way to the left, ending on a ring.
    seg_along(e, float2(-145, -39), float2(-250, -39), 0.0, best, along);
    seg_along(e, float2(-250, -39), float2(-330, 41), 105.0, best, along);
    seg_along(e, float2(-330, 41), float2(-470, 41), 218.1, best, along);
    if (best < tb) { tb = best; ta = along; tid = 2.0; }
    best = 1e5;
    // South via, down to the bottom edge.
    seg_along(e, float2(-53, 122), float2(-53, 230), 0.0, best, along);
    seg_along(e, float2(-53, 230), float2(-143, 320), 108.0, best, along);
    seg_along(e, float2(-143, 320), float2(-143, 1400), 235.3, best, along);
    if (best < tb) { tb = best; ta = along; tid = 3.0; }
    best = 1e5;
    // East ring, out to the right edge.
    seg_along(e, float2(136, -6), float2(250, -6), 0.0, best, along);
    seg_along(e, float2(250, -6), float2(310, 54), 114.0, best, along);
    seg_along(e, float2(310, 54), float2(3000, 54), 198.9, best, along);
    if (best < tb) { tb = best; ta = along; tid = 4.0; }

    float endRing = abs(length(e - float2(-470, 41)) - 9.0) - 3.0;

    // ---- the board, kept clear of the eye and its traces
    float3 bd = buses(b, t);
    float fade = smoothstep(0.30, 0.75, b.x / (W * unit)) * smoothstep(240.0, 340.0, toEye);
    fade *= smoothstep(14.0, 30.0, min(tb, endRing) * EYE_SCALE) * board;
    float cov = saturate(0.5 - (bd.x - TRACE_HALF) / unit);
    float vcov = saturate(0.5 - bd.z / unit);
    col += MILVA_GREY * BOARD_LEVEL * max(cov, vcov) * fade;
    float sig = bd.y * fade * glow;
    float halo = exp(-max(bd.x - TRACE_HALF, 0.0) / 4.0);
    col += lerp(MILVA_ORANGE, MILVA_YELLOW, bd.y) * sig * (cov * 0.55 + halo * 0.18);

    float tcov = saturate(0.5 - (min(tb - EYE_TRACE_HALF, endRing)) / eu);
    col = lerp(col, MILVA_GREY * EYE_TRACE_LEVEL + col * 0.4, tcov);
    float es = signal(ta, t, hash12(float2(tid, 11.0)), EYE_PERIOD, tid + 40.0);
    float ehalo = exp(-max(tb - EYE_TRACE_HALF, 0.0) / 7.0);
    col += lerp(MILVA_ORANGE, MILVA_YELLOW, es) * es * glow * (tcov * 0.6 + ehalo * 0.2);

    // ---- the eye: legs, lid bar, half-disc body, pupil, pads
    float ink = 1e5;
    ink = min(ink, seg(e, float2(13, -12), float2(35, -69)));
    ink = min(ink, seg(e, float2(-48, 6), float2(-46, -46)));
    ink = min(ink, seg(e, float2(-46, -46), float2(-130, -38)));
    ink = min(ink, seg(e, float2(-25, 54), float2(-53, 122)));
    ink = min(ink, seg(e, float2(50, 46), float2(117, 54)));
    ink = min(ink, seg(e, float2(117, 54), float2(132, 10)));
    ink = min(ink, seg(e, float2(-109, 15), float2(95, -14)));
    ink -= STROKE_HALF;

    // A slow blink now and then: the body folds up into the lid bar.
    float bp = (frac(t / BLINK_EVERY + 0.37) * BLINK_EVERY) / BLINK_SECONDS;
    float lid = saturate(1.0 - abs(2.0 * bp - 1.0));
    lid = lid * lid * (3.0 - 2.0 * lid);
    float2 q = rot(e, BAR_ANGLE);
    q.y /= max(1.0 - lid, 0.02);
    float body = max(length(q) - BODY_R, -q.y);
    float pupilX = 23.0 * 0.4 + sin(t * 0.11 + 1.3) * 34.0 * 0.6;
    float pupil = max(length(q - float2(pupilX, 0.0)) - PUPIL_R, -q.y);
    body = max(body, -pupil);
    body = lid > 0.98 ? 1e5 : body;
    ink = min(ink, body);

    float padN = length(e - float2(35, -69)) - 18.0;
    float padW = abs(length(e - float2(-145, -39)) - 11.0) - 4.0;
    float padS = length(e - float2(-53, 122)) - 18.0;
    float padE = abs(length(e - float2(136, -6)) - 11.0) - 4.0;
    ink = min(ink, min(min(padN, padW), min(padS, padE)));

    float ecov = saturate(0.5 - ink / eu);
    col = lerp(col, MILVA_GREY * EYE_LEVEL, ecov);

    // A pad warms up as a signal leaves it, then cools as the signal travels on.
    float4 lit = float4(
        signal(0.0, t, hash12(float2(1.0, 11.0)), EYE_PERIOD, 41.0),
        signal(0.0, t, hash12(float2(2.0, 11.0)), EYE_PERIOD, 42.0),
        signal(0.0, t, hash12(float2(3.0, 11.0)), EYE_PERIOD, 43.0),
        signal(0.0, t, hash12(float2(4.0, 11.0)), EYE_PERIOD, 44.0));
    float4 pcv = saturate(0.5 - float4(padN, padW, padS, padE) / eu);
    float warm = dot(lit, pcv);
    col = lerp(col, lerp(MILVA_ORANGE, MILVA_YELLOW, warm) * PAD_GLOW * glow, saturate(warm * glow));

    // The pupil keeps a faint warm light that breathes very slowly.
    float pcov = saturate(0.5 - pupil / eu) * (lid > 0.98 ? 0.0 : 1.0);
    float breathe = 0.55 + 0.45 * sin(t * 0.21);
    col += MILVA_YELLOW * pcov * (0.10 + 0.08 * breathe) * glow;

    // A little noise keeps the dark gradients free of bands.
    col += (hash12(px) - 0.5) * DITHER;
    return col;
}
