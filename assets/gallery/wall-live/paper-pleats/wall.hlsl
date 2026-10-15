// Paper pleats: a sheet of paper folded along curved creases, rings of
// mountain and valley folds like the pleated paper studies of the Bauhaus.
// A slow wave of folding runs outward through the rings, so each curved
// band turns its light from bright to shade along its length under a lamp
// that swings slowly overhead. Near the icons the sheet lies almost flat.

float pp_hash(float2 p)
{
    float3 p3 = frac(p.xyx * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
}

// A triangle wave in -1..1 with corners rounded by e (in radians of the
// wave), so creases are crisp yet never stair-stepped; dv is its slope.
float pp_tri(float v, float e, out float dv)
{
    float th = 6.2831853 * v;
    float s = sin(th);
    float k = sqrt(1.0 - e * e);
    float nrm = 1.0 / asin(k);
    dv = k * cos(th) * 6.2831853 * nrm / sqrt(1.0 - k * k * s * s);
    return asin(k * s) * nrm;
}

float3 wall(float2 px)
{
    float2 uv = px / resolution.y;
    float aspect = resolution.x / resolution.y;
    float t = time * speed;
    float xr = uv.x / aspect;

    // The pleats are rings around a point beyond the lower right corner:
    // curved creases, mountain and valley in turn, a little wider outward.
    float2 cen = float2(aspect * 1.02 + 0.05 * sin(t * 0.006), 1.12 + 0.04 * cos(t * 0.005));
    float2 dv = uv - cen;
    float r = length(dv);
    float2 rd = dv / max(r, 1e-4);
    float w = 0.14 / pleats;
    float u = pow(r, 0.8) / w;
    float dudr = 0.8 * pow(max(r, 1e-4), -0.2) / w;
    float pxu = 1.0 / resolution.y;
    float e = min(6.2831853 * 1.5 * pxu * dudr, 0.5);

    // How far the sheet is folded: flat by the icons, and a slow wave of
    // folding that runs outward through the rings.
    float fold = smoothstep(0.04, 0.45, xr) * (0.6 + 0.4 * sin(t * 0.02 - r * 2.2));
    float amp = 0.02 * depth * fold;

    float dz;
    float tu = pp_tri(u, e, dz);
    float z = amp * tu;
    float2 gz = amp * dz * dudr * rd;
    // The whole sheet bows into a gentle saddle that slowly turns over, so
    // the light runs along each ring from bright to shade.
    float2 sc = uv - float2(aspect * 0.62, 0.5);
    float bow = 0.30 * depth * sin(t * 0.008 + 0.8) * smoothstep(0.04, 0.45, xr);
    gz += bow * float2(sc.y, sc.x);

    // Light from a lamp above the sheet that swings slowly from side to side.
    float la = -1.75 + 0.45 * sin(t * 0.009);
    float3 L = normalize(float3(cos(la), sin(la), 0.75));
    float3 n = normalize(float3(-gz, 1.0));
    float diff = dot(n, L);
    float flatL = L.z;

    float3 paperC = lerp(float3(0.82, 0.79, 0.74), float3(0.20, 0.23, 0.27), shade);
    // A warm lamp on the left, a cool sky fill from the right: facets that
    // turn away from the lamp go a little blue.
    float3 lightC = lerp(float3(1.03, 0.99, 0.93), float3(0.92, 0.96, 1.03), smoothstep(-0.6, 0.6, n.x + n.y * 0.4));
    // Lit relative to a flat sheet, so the unfolded part keeps its true tone.
    float lit = 1.0 + (diff - flatL) * 1.1;
    float3 col = paperC * lit * lightC;
    // Valleys hold a little shade.
    col *= 1.0 - 0.10 * saturate(-tu) * fold - 0.12 * smoothstep(-0.85, -1.0, tu) * fold;
    // Mountain creases catch a hairline of light.
    col += float3(1.0, 0.98, 0.94) * smoothstep(0.93, 1.0, tu) * 0.06 * fold;
    // A soft brighter spot where the lamp falls, broad and calm.
    float2 spot = float2(aspect * 0.78, 0.30);
    col *= 0.86 + 0.20 * exp(-dot(uv - spot, uv - spot) * 1.4);

    // Fibres in the paper.
    col *= 0.985 + 0.03 * pp_hash(floor(px / 2.0));
    return col;
}
