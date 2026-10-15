// Deep-sea jellyfish: a glass bell lit from within, a rim of light, ruffled violet
// arms and long hair-fine tentacles trailing in the dark; a little marine snow.

float hash21(float2 p)
{
    p = frac(p * float2(123.34, 456.21));
    p += dot(p, p + 45.32);
    return frac(p.x * p.y);
}

float vnoise(float2 p)
{
    float2 i = floor(p);
    float2 f = frac(p);
    f = f * f * (3.0 - 2.0 * f);
    float a = hash21(i);
    float b = hash21(i + float2(1.0, 0.0));
    float c = hash21(i + float2(0.0, 1.0));
    float d = hash21(i + float2(1.0, 1.0));
    return lerp(lerp(a, b, f.x), lerp(c, d, f.x), f.y);
}

// A line of half-width `w` at distance `d`, anti-aliased over `aa`.
float stroke(float d, float w, float aa)
{
    return smoothstep(w + aa, max(w - aa, 0.0), d) * saturate(w / aa + 0.35);
}

float3 wall(float2 px)
{
    float pxs = 1.0 / resolution.y;
    float2 p = (px - 0.5 * resolution) * pxs;
    p.y = -p.y;

    float3 cyan = float3(0.32, 0.88, 1.0);
    float3 violet = float3(0.72, 0.42, 1.0);
    float3 col = 0.0;

    // The jellyfish's own frame: it leans as it swims up and to the right.
    float2 B = float2(-0.16, 0.17);
    float2 up = normalize(float2(0.3 * drift, 1.0));
    float2 rt = float2(up.y, -up.x);
    float2 v = p - B;
    float2 l = float2(dot(v, rt), dot(v, up));

    float a = 0.145;
    float b = 0.14;
    float xn = l.x / a;
    // The margin: the bell's lower edge, curving up at the sides, softly scalloped.
    float marginY = -0.01 + 0.035 * xn * xn + 0.0016 * cos(xn * 31.4159);
    // The dome narrows a little at the bottom, like a bell.
    float2 ls = float2(l.x / (1.0 - 0.1 * saturate(-l.y / b + 0.25)), l.y);
    float e = length(ls / float2(a, b)) - 1.0;
    float above = smoothstep(marginY - pxs, marginY + pxs, l.y);

    // The bell: an outline brightest at the crown, a glassy fill brighter towards
    // its edge, and the inner bell showing through.
    float de = abs(e) * b;
    float crown = 0.45 + 0.55 * saturate(l.y / b + 0.2);
    float outline = stroke(de, 0.0013 + 0.0012 * crown, pxs) * above;
    float inside = step(e, 0.0) * above;
    float fres = smoothstep(-0.6, 0.0, e);
    col += cyan * outline * crown * 0.9;
    col += cyan * inside * (0.008 + 0.075 * fres * fres * fres) * (0.6 + 0.4 * crown);
    col += cyan * 0.06 * crown * exp(-max(e, 0.0) * b / 0.008) * step(0.0, e) * above;
    float e2 = length((ls - float2(0.0, -0.012)) / float2(a * 0.8, b * 0.74)) - 1.0;
    col += cyan * inside * stroke(abs(e2) * b * 0.74, 0.0008, pxs) * 0.28 * step(marginY + 0.012, l.y);

    // Meridian canals running down the dome.
    float yy = saturate(l.y / b);
    float half = a * sqrt(max(1.0 - yy * yy, 0.02));
    float s = l.x / half;
    float mer = abs(frac(s * 4.0 + 0.5) - 0.5) / 4.0 * half;
    col += cyan * inside * stroke(mer, 0.0006, pxs) * 0.2 * smoothstep(-0.1, 0.4, yy);

    // Gonads: four soft violet lobes glowing inside the crown.
    float gon = 0.0;
    [unroll]
    for (int g = 0; g < 4; g++)
    {
        float fg = float(g);
        float side = fg - 1.5;
        float2 gc = float2(side * 0.036, 0.05 - 0.012 * abs(side));
        float2 gq = l - gc;
        float ang = side * 0.35;
        gq = float2(gq.x * cos(ang) - gq.y * sin(ang), gq.x * sin(ang) + gq.y * cos(ang));
        float gr = length(gq * float2(1.0, 0.8)) - 0.014;
        // a horseshoe, open towards the margin
        float open = smoothstep(-0.006, -0.012, gq.y) * smoothstep(0.009, 0.004, abs(gq.x));
        float front = 0.6 + 0.4 * abs(side) / 1.5;
        gon += front * stroke(abs(gr), 0.0017, pxs) * (1.0 - open) * (0.55 + 0.45 * vnoise(gq * 260.0));
        gon += front * 0.18 * exp(-abs(gr) / 0.004) * (1.0 - open);
    }
    gon += 0.25 * exp(-length((l - float2(0.0, 0.035)) * float2(1.0, 1.6)) / 0.03);
    col += violet * gon * inside * 0.7;

    // Small lights along the margin.
    float rimD = abs(l.y - marginY);
    float onRim = step(abs(xn), 0.97);
    float dots = frac(xn * 8.0 + 0.5) - 0.5;
    float2 dq = float2(dots * a / 8.0, rimD);
    col += float3(0.8, 1.0, 1.0) * onRim * (stroke(length(dq), 0.0016, pxs) * 0.9 + 0.12 * exp(-length(dq) / 0.004));
    col += cyan * onRim * stroke(rimD, 0.0008, pxs) * 0.45;

    float below = marginY - l.y; // distance under the margin

    // Ruffled oral arms in violet.
    [unroll]
    for (int k = 0; k < 4; k++)
    {
        float fk = float(k);
        float len = 0.22 + 0.08 * hash21(float2(fk, 2.0));
        float bk = l.y < -0.01 ? -0.01 - l.y : -1.0;
        float t = saturate(bk / len);
        float x0 = (fk - 1.5) * 0.014;
        float cx = x0 + 0.028 * t * sin(t * 6.0 + fk * 1.7) + (fk - 1.5) * 0.018 * t - drift * 0.04 * t * t;
        float hw = 0.013 * (1.0 - 0.8 * t) * (0.65 + 0.35 * sin(bk * 160.0 + fk * 2.0)) + 0.0008;
        float dx = abs(l.x - cx);
        float alive = step(0.0, bk) * step(bk, len) * smoothstep(1.0, 0.75, t);
        float edgeL = stroke(abs(dx - hw), 0.0008, pxs);
        float fill = smoothstep(hw + pxs, hw - pxs, dx) * (0.35 + 0.65 * vnoise(float2(dx * 600.0, bk * 120.0)));
        col += violet * alive * (edgeL * 0.5 + fill * 0.1) * (1.0 - 0.6 * t);
    }

    // Hair-fine tentacles from the margin, trailing behind and fading.
    [unroll]
    for (int i = 0; i < 16; i++)
    {
        float fi = float(i);
        float u = ((fi + 0.2 + 0.6 * hash21(float2(fi, 1.0))) / 16.0) * 2.0 - 1.0;
        u *= 0.97;
        float xi = u * a;
        float yi = -0.01 + 0.035 * u * u;
        float dy = yi - l.y;
        float len = 0.3 + 0.35 * hash21(float2(fi, 7.0));
        float amp = 0.012 + 0.02 * hash21(float2(fi, 3.0));
        float k1 = 5.0 + 9.0 * hash21(float2(fi, 5.0));
        float ph = 6.28 * hash21(float2(fi, 11.0));
        float trail = -drift * 0.18;
        float sw = sin(dy * k1 + ph);
        float gx = xi + u * 0.06 * dy + amp * sw * dy * 3.0 + trail * dy * dy;
        float gd = u * 0.06 + amp * 3.0 * (sw + dy * k1 * cos(dy * k1 + ph)) + 2.0 * trail * dy;
        float d = abs(l.x - gx) / sqrt(1.0 + gd * gd);
        float t = dy / len;
        float alive = step(0.0, dy) * smoothstep(1.0, 0.35, t);
        col += cyan * alive * stroke(d, 0.00055, pxs) * (0.7 - 0.4 * t);
    }

    // Marine snow.
    float2 sg = p * 22.0;
    float2 cell = floor(sg);
    float2 off = float2(hash21(cell + 1.0), hash21(cell + 23.0)) * 0.8 + 0.1;
    float sd = length(sg - cell - off) / 22.0;
    float sz = 0.0006 + 0.0012 * hash21(cell + 4.0);
    col += float3(0.6, 0.8, 0.9) * step(hash21(cell + 31.0), 0.22) * smoothstep(sz + pxs, sz - pxs, sd) * (0.1 + 0.25 * hash21(cell + 8.0));

    return max(col * glow - 0.002, 0.0);
}
