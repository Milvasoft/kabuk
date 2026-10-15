// Quiet orrery: a small sun and six planets on fine orbits, seen from a little above
// the plane. Each planet is lit from the sun, so it shows its phase, and leaves a
// fading arc of light behind it on its orbit; the whole model turns and drifts slowly.

static const int PLANETS = 6;
static const float TAU = 6.2831853;

// Orbit radius (in screen heights, before the tilt), planet radius, start angle.
static const float3 ORBIT[PLANETS] = {
    float3(0.1, 0.0052, 0.3),
    float3(0.16, 0.0078, 2.1),
    float3(0.225, 0.0082, 4.0),
    float3(0.29, 0.0064, 5.3),
    float3(0.41, 0.017, 1.2),
    float3(0.54, 0.0135, 3.3),
};

static const float3 HUE[PLANETS] = {
    float3(0.72, 0.68, 0.64),   // grey
    float3(0.95, 0.85, 0.62),   // cream
    float3(0.35, 0.6, 1.0),     // blue
    float3(0.95, 0.45, 0.28),   // rust
    float3(0.9, 0.72, 0.52),    // banded tan
    float3(0.95, 0.82, 0.5),    // ringed gold
};

float2 rot(float2 p, float c, float s)
{
    return float2(c * p.x - s * p.y, s * p.x + c * p.y);
}

float3 wall(float2 px)
{
    float2 uv = (px - 0.5 * resolution) / resolution.y;
    float aspect = resolution.x / resolution.y;
    float pix = 1.0 / resolution.y;
    float t = time * years;

    // The view: the model's centre wanders, it leans and turns slowly.
    float2 centre = float2(0.16 * (aspect - 1.0) * sin(time * 0.009 + 0.5), 0.05 * sin(time * 0.013 + 1.0));
    float cosi = 0.36 + 0.1 * sin(time * 0.011);
    float sini = sqrt(1.0 - cosi * cosi);
    float pa = -0.25 + 0.2 * sin(time * 0.006);
    float cp = cos(pa), sp = sin(pa);
    float2 p = rot(uv - centre, cp, -sp);
    float2 disc = float2(p.x, p.y / cosi);
    float r = length(disc);
    float th = atan2(disc.y, disc.x);
    float fw = fwidth(r);

    float3 col = 0;

    // The sun: a warm disc that breathes, inside a soft corona.
    float sr = length(uv - centre);
    float breath = 0.75 + 0.25 * sin(time * 0.21);
    float sunDisc = saturate((0.014 - sr) / pix + 0.5);
    col += lerp(float3(1.0, 0.55, 0.15), float3(1.0, 0.9, 0.7), saturate(1.0 - sr / 0.014)) * sunDisc * breath;
    col += float3(1.0, 0.5, 0.15) * (0.3 * exp(-sr / 0.014) + 0.07 * exp(-sr / 0.06)) * breath * (1.0 - sunDisc);

    [unroll]
    for (int i = 0; i < PLANETS; i++)
    {
        float a = ORBIT[i].x;
        float size = ORBIT[i].y;
        // Kepler's third law: the period grows with the orbit's radius to the power 1.5.
        float period = 26.0 * pow(a / 0.1, 1.5);
        float ang = ORBIT[i].z + TAU * t / period;

        // The orbit, with a fading arc of light behind the planet.
        float behind = frac((ang - th) / TAU);
        float arc = exp(-behind * 7.0);
        float ring = saturate(1.0 - abs(r - a) / (1.2 * fw));
        col += lerp(float3(0.5, 0.6, 0.8), HUE[i], arc) * ring * (0.07 * orbits + 0.5 * arc);

        // The planet, a sphere lit from the sun: its place in 3D (x, y on the screen
        // before the turn, z towards us), and the light's direction from it.
        float2 flat = a * float2(cos(ang), sin(ang));
        float3 pos = float3(flat.x, flat.y * cosi, -flat.y * sini);
        float3 L = normalize(-pos);
        L.xy = rot(L.xy, cp, sp);
        float2 at = centre + rot(pos.xy, cp, sp);
        float2 d = (uv - at) / size;
        float dd = dot(d, d);
        float cover = saturate((1.0 - sqrt(dd)) * size / pix + 0.5);
        float3 n = float3(d.x, d.y, sqrt(saturate(1.0 - dd)));
        // y grows downwards on the screen; the sphere's normal uses the screen's axes.
        float lit = saturate(dot(n, float3(L.x, L.y, -L.z)));
        float3 surface = HUE[i];
        surface *= (i == 4) ? 0.8 + 0.2 * sin(d.y * 14.0 + d.x * 2.0) : 1.0;
        float rimLight = pow(1.0 - n.z, 3.0) * 0.18;
        col = lerp(col, surface * (0.03 + rimLight + 0.95 * pow(lit, 0.8)), cover);

        // Saturn's ring, tilted with the plane.
        if (i == 5)
        {
            float2 q = rot(uv - at, cp, -sp);
            float rr = length(float2(q.x, q.y / cosi)) / size;
            float band = smoothstep(1.45, 1.55, rr) * (1.0 - smoothstep(2.1, 2.25, rr));
            // The part of the ring behind the planet is hidden by it.
            // The near half lies over the planet, the far half behind it.
            float hidden = step(q.y, 0.0) * cover;
            col = lerp(col, float3(0.85, 0.75, 0.5) * 0.45, band * (1.0 - hidden));
        }
        // Earth's moon.
        if (i == 2)
        {
            float ma = t * TAU / 6.0;
            float2 m = 0.022 * float2(cos(ma), sin(ma));
            float2 mat = at + rot(float2(m.x, m.y * cosi), cp, sp);
            float md = length(uv - mat);
            col += float3(0.8, 0.8, 0.82) * saturate((0.0026 - md) / pix + 0.5) * 0.7;
        }
    }
    return col;
}
