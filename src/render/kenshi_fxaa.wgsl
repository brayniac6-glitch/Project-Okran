// Kenshi's FXAA (data/materials/post/FXAA.hlsl, FXAA.material): NVIDIA FXAA 3.11's PC quality
// path (Fxaa3_11.h) with FXAA_GREEN_AS_LUMA, FXAA_QUALITY__PRESET 12, no gather, no discard;
// subpix 0.75, edge threshold 0.166, edge threshold min 0.0833. It reads the 8-bit composite
// (display values) bilinearly with border addressing (Ogre's border colour, black).

#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

@group(0) @binding(0) var rt: texture_2d<f32>;

const SUBPIX: f32 = 0.75;
const EDGE_THRESHOLD: f32 = 0.166;
const EDGE_THRESHOLD_MIN: f32 = 0.0833;
// FXAA_QUALITY__PRESET 12
const P0: f32 = 1.0;
const P1: f32 = 1.5;
const P2: f32 = 2.0;
const P3: f32 = 4.0;
const P4: f32 = 12.0;

fn texel(p: vec2<i32>) -> vec4<f32> {
    let d = vec2<i32>(textureDimensions(rt));
    if (any(p < vec2<i32>(0)) || any(p >= d)) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    return textureLoad(rt, p, 0);
}

// SampleLevel(.., 0) with bilinear filtering and border addressing
fn tex_top(uv: vec2<f32>) -> vec4<f32> {
    let d = vec2<f32>(textureDimensions(rt));
    let p = uv * d - 0.5;
    let f = floor(p);
    let w = p - f;
    let i = vec2<i32>(f);
    let a = mix(texel(i), texel(i + vec2<i32>(1, 0)), w.x);
    let b = mix(texel(i + vec2<i32>(0, 1)), texel(i + vec2<i32>(1, 1)), w.x);
    return mix(a, b, w.y);
}

fn tex_off(uv: vec2<f32>, o: vec2<f32>, rcp: vec2<f32>) -> vec4<f32> {
    return tex_top(uv + o * rcp);
}

fn luma(c: vec4<f32>) -> f32 {
    return c.y;
}

@fragment
fn fxaa(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let rcp = 1.0 / vec2<f32>(textureDimensions(rt));
    var posM = in.uv;
    let rgbyM = tex_top(posM);
    let lumaM = rgbyM.y;
    var lumaS = luma(tex_off(posM, vec2<f32>(0.0, 1.0), rcp));
    let lumaE = luma(tex_off(posM, vec2<f32>(1.0, 0.0), rcp));
    var lumaN = luma(tex_off(posM, vec2<f32>(0.0, -1.0), rcp));
    let lumaW = luma(tex_off(posM, vec2<f32>(-1.0, 0.0), rcp));

    let maxSM = max(lumaS, lumaM);
    let minSM = min(lumaS, lumaM);
    let maxESM = max(lumaE, maxSM);
    let minESM = min(lumaE, minSM);
    let maxWN = max(lumaN, lumaW);
    let minWN = min(lumaN, lumaW);
    let rangeMax = max(maxWN, maxESM);
    let rangeMin = min(minWN, minESM);
    let rangeMaxScaled = rangeMax * EDGE_THRESHOLD;
    let range = rangeMax - rangeMin;
    let rangeMaxClamped = max(EDGE_THRESHOLD_MIN, rangeMaxScaled);
    if (range < rangeMaxClamped) {
        return rgbyM;
    }

    let lumaNW = luma(tex_off(posM, vec2<f32>(-1.0, -1.0), rcp));
    let lumaSE = luma(tex_off(posM, vec2<f32>(1.0, 1.0), rcp));
    let lumaNE = luma(tex_off(posM, vec2<f32>(1.0, -1.0), rcp));
    let lumaSW = luma(tex_off(posM, vec2<f32>(-1.0, 1.0), rcp));

    let lumaNS = lumaN + lumaS;
    let lumaWE = lumaW + lumaE;
    let subpixRcpRange = 1.0 / range;
    let subpixNSWE = lumaNS + lumaWE;
    let edgeHorz1 = (-2.0 * lumaM) + lumaNS;
    let edgeVert1 = (-2.0 * lumaM) + lumaWE;
    let lumaNESE = lumaNE + lumaSE;
    let lumaNWNE = lumaNW + lumaNE;
    let edgeHorz2 = (-2.0 * lumaE) + lumaNESE;
    let edgeVert2 = (-2.0 * lumaN) + lumaNWNE;
    let lumaNWSW = lumaNW + lumaSW;
    let lumaSWSE = lumaSW + lumaSE;
    let edgeHorz4 = (abs(edgeHorz1) * 2.0) + abs(edgeHorz2);
    let edgeVert4 = (abs(edgeVert1) * 2.0) + abs(edgeVert2);
    let edgeHorz3 = (-2.0 * lumaW) + lumaNWSW;
    let edgeVert3 = (-2.0 * lumaS) + lumaSWSE;
    let edgeHorz = abs(edgeHorz3) + edgeHorz4;
    let edgeVert = abs(edgeVert3) + edgeVert4;
    let subpixNWSWNESE = lumaNWSW + lumaNESE;
    var lengthSign = rcp.x;
    let horzSpan = edgeHorz >= edgeVert;
    let subpixA = subpixNSWE * 2.0 + subpixNWSWNESE;
    if (!horzSpan) {
        lumaN = lumaW;
        lumaS = lumaE;
    } else {
        lengthSign = rcp.y;
    }
    let subpixB = (subpixA * (1.0 / 12.0)) - lumaM;
    let gradientN = lumaN - lumaM;
    let gradientS = lumaS - lumaM;
    var lumaNN = lumaN + lumaM;
    let lumaSS = lumaS + lumaM;
    let pairN = abs(gradientN) >= abs(gradientS);
    let gradient = max(abs(gradientN), abs(gradientS));
    if (pairN) {
        lengthSign = -lengthSign;
    }
    let subpixC = clamp(abs(subpixB) * subpixRcpRange, 0.0, 1.0);

    var posB = posM;
    var offNP: vec2<f32>;
    offNP.x = select(rcp.x, 0.0, !horzSpan);
    offNP.y = select(rcp.y, 0.0, horzSpan);
    if (!horzSpan) {
        posB.x += lengthSign * 0.5;
    }
    if (horzSpan) {
        posB.y += lengthSign * 0.5;
    }
    var posN = posB - offNP * P0;
    var posP = posB + offNP * P0;
    let subpixD = ((-2.0) * subpixC) + 3.0;
    var lumaEndN = luma(tex_top(posN));
    let subpixE = subpixC * subpixC;
    var lumaEndP = luma(tex_top(posP));
    if (!pairN) {
        lumaNN = lumaSS;
    }
    let gradientScaled = gradient * 1.0 / 4.0;
    let lumaMM = lumaM - lumaNN * 0.5;
    let subpixF = subpixD * subpixE;
    let lumaMLTZero = lumaMM < 0.0;
    lumaEndN -= lumaNN * 0.5;
    lumaEndP -= lumaNN * 0.5;
    var doneN = abs(lumaEndN) >= gradientScaled;
    var doneP = abs(lumaEndP) >= gradientScaled;
    if (!doneN) {
        posN -= offNP * P1;
    }
    var doneNP = (!doneN) || (!doneP);
    if (!doneP) {
        posP += offNP * P1;
    }
    // the search steps P2, P3 and P4 (FXAA_QUALITY__PS 5)
    let steps = array<f32, 3>(P2, P3, P4);
    for (var k = 0; k < 3; k++) {
        if (!doneNP) {
            break;
        }
        if (!doneN) {
            lumaEndN = luma(tex_top(posN)) - lumaNN * 0.5;
        }
        if (!doneP) {
            lumaEndP = luma(tex_top(posP)) - lumaNN * 0.5;
        }
        doneN = abs(lumaEndN) >= gradientScaled;
        doneP = abs(lumaEndP) >= gradientScaled;
        if (!doneN) {
            posN -= offNP * steps[k];
        }
        doneNP = (!doneN) || (!doneP);
        if (!doneP) {
            posP += offNP * steps[k];
        }
    }

    var dstN = posM.x - posN.x;
    var dstP = posP.x - posM.x;
    if (!horzSpan) {
        dstN = posM.y - posN.y;
        dstP = posP.y - posM.y;
    }
    let goodSpanN = (lumaEndN < 0.0) != lumaMLTZero;
    let spanLength = dstP + dstN;
    let goodSpanP = (lumaEndP < 0.0) != lumaMLTZero;
    let spanLengthRcp = 1.0 / spanLength;
    let directionN = dstN < dstP;
    let dst = min(dstN, dstP);
    let goodSpan = select(goodSpanP, goodSpanN, directionN);
    let subpixG = subpixF * subpixF;
    let pixelOffset = (dst * (-spanLengthRcp)) + 0.5;
    let subpixH = subpixG * SUBPIX;
    let pixelOffsetGood = select(0.0, pixelOffset, goodSpan);
    let pixelOffsetSubpix = max(pixelOffsetGood, subpixH);
    if (!horzSpan) {
        posM.x += pixelOffsetSubpix * lengthSign;
    }
    if (horzSpan) {
        posM.y += pixelOffsetSubpix * lengthSign;
    }
    return vec4<f32>(tex_top(posM).xyz, lumaM);
}
