#!/usr/bin/env python3
"""NET780 VDF (web地球号 ドライブレコーダー映像) 抽出ツール — リファレンス実装。

フォーマット仕様: docs/vdf-format.md

使い方:
    python3 scripts/vdf_extract.py <file.vdf> [outdir]

出力 (outdir、default カレント):
    front.mp4 / rear.mp4     前方・後方映像 (VFR、フレーム実時刻で mux)
    speed_rpm.csv            速度 (km/h) + エンジン回転数 (rpm)
    g_sensor.csv             Gセンサー 3軸 (G単位)
    gps.csv                  GPS (緯度/経度/方位)
    events.csv               イベント発生時刻

依存: Python 3.9+ 標準ライブラリのみ (ffmpeg 不要、MP4 は自前 mux)。
時刻はファイル内表現のまま = JST。
"""
import csv
import datetime
import json
import os
import struct
import sys

TIMESCALE = 1_000_000  # MP4 timescale (µs 精度)


def fmt_ts(ts, sub_us):
    """ファイル内 ts は JST 壁時計を epoch に入れた値なので utc で読む"""
    t = datetime.datetime.utcfromtimestamp(ts)
    return t.strftime("%Y-%m-%d %H:%M:%S") + f".{sub_us // 10000:02d}"


def parse_vdf(data):
    if data[:6] != b"NET780":
        raise ValueError("not a NET780 VDF file")
    vehicle = data[0x34:0x54].split(b"\0")[0].decode(errors="replace")
    driver = data[0x54:0x74].split(b"\0")[0].decode(errors="replace")

    frames = {0: [], 1: []}  # ch -> [(ts, sub, key, w, h, payload)]
    g_recs, sr_recs, gps_recs, ev_recs = [], [], [], []

    pos, end = 0x300, len(data)
    while pos + 24 <= end:
        marker = data[pos]
        if marker == 0xFC and data[pos + 2] in (0, 1):
            ch = data[pos + 1]
            key = data[pos + 3]
            w, h = struct.unpack_from("<HH", data, pos + 4)
            size = struct.unpack_from("<I", data, pos + 12)[0]
            ts, sub = struct.unpack_from("<II", data, pos + 24)
            frames.setdefault(ch, []).append(
                (ts, sub, key, w, h, data[pos + 40 : pos + 40 + size])
            )
            pos += 40 + size
        elif marker == 0xFD:
            t = data[pos + 1]
            if t == 0x00:  # Gセンサー (mG)
                x, y, z = struct.unpack_from("<hhh", data, pos + 2)
                ts, sub = struct.unpack_from("<II", data, pos + 16)
                g_recs.append((ts, sub, x, y, z))
                pos += 32
            elif t == 0x01:  # GPS
                fix = chr(data[pos + 3])
                lat, lon = (v / 1e7 for v in struct.unpack_from("<ii", data, pos + 4))
                _, heading = struct.unpack_from("<HH", data, pos + 12)
                ts, sub = struct.unpack_from("<II", data, pos + 24)
                gps_recs.append((ts, sub, fix, lat, lon, heading))
                pos += 40
            elif t == 0x02:  # 速度 (×0.01km/h) + rpm
                spd, rpm = struct.unpack_from("<HH", data, pos + 4)
                ts, sub = struct.unpack_from("<II", data, pos + 16)
                sr_recs.append((ts, sub, spd, rpm))
                pos += 32
            elif t == 0x03:  # sync
                pos += 24
            else:
                raise ValueError(f"unknown fd record type {t} at {pos:#x}")
        elif marker == 0xFE:  # イベント発生マーカー
            ts, sub = struct.unpack_from("<II", data, pos + 16)
            code = struct.unpack_from("<H", data, pos + 2)[0]
            ev_recs.append((ts, sub, code))
            pos += 32
        elif marker == 0xFF:  # 末尾シークインデックス = 終端
            break
        else:
            raise ValueError(f"unexpected marker {marker:#x} at {pos:#x}")

    return {
        "vehicle": vehicle,
        "driver": driver,
        "frames": frames,
        "g": g_recs,
        "speed_rpm": sr_recs,
        "gps": gps_recs,
        "events": ev_recs,
    }


# ---------------- 最小 MP4 (VFR) muxer ----------------

def _box(tag, payload):
    return struct.pack(">I", 8 + len(payload)) + tag + payload


def _full(tag, ver, flags, payload):
    return _box(tag, bytes([ver]) + flags.to_bytes(3, "big") + payload)


def _split_nals(buf):
    out, i = [], 0
    while True:
        j = buf.find(b"\x00\x00\x00\x01", i)
        if j < 0:
            break
        k = buf.find(b"\x00\x00\x00\x01", j + 4)
        out.append(buf[j + 4 : k if k >= 0 else len(buf)])
        if k < 0:
            break
        i = k
    return out


def write_mp4(frames, out_path):
    """frames: [(ts, sub, key, w, h, annexb_payload)] — フレーム実時刻で VFR mux"""
    sps = pps = None
    mdat = bytearray()
    sizes, keyflags = [], []
    times = [ts + sub / 1e6 for ts, sub, *_ in frames]
    for _, _, key, _, _, payload in frames:
        sample = bytearray()
        for nal in _split_nals(payload):
            nt = nal[0] & 0x1F
            if nt == 7:
                sps = nal
                continue
            if nt == 8:
                pps = nal
                continue
            sample += struct.pack(">I", len(nal)) + nal
        mdat += sample
        sizes.append(len(sample))
        keyflags.append(bool(key))
    w_px, h_px = frames[0][3], frames[0][4]

    durs = [max(1, round((times[i + 1] - times[i]) * TIMESCALE)) for i in range(len(times) - 1)]
    durs.append(durs[-1] if durs else TIMESCALE // 10)
    total = sum(durs)

    ftyp = _box(b"ftyp", b"isom" + struct.pack(">I", 512) + b"isomiso2avc1mp41")
    runs = []
    for d in durs:
        if runs and runs[-1][1] == d:
            runs[-1][0] += 1
        else:
            runs.append([1, d])
    stts = _full(b"stts", 0, 0, struct.pack(">I", len(runs)) + b"".join(struct.pack(">II", c, d) for c, d in runs))
    stss_e = [i + 1 for i, k in enumerate(keyflags) if k]
    stss = _full(b"stss", 0, 0, struct.pack(">I", len(stss_e)) + b"".join(struct.pack(">I", s) for s in stss_e))
    stsc = _full(b"stsc", 0, 0, struct.pack(">I", 1) + struct.pack(">III", 1, len(sizes), 1))
    stsz = _full(b"stsz", 0, 0, struct.pack(">II", 0, len(sizes)) + b"".join(struct.pack(">I", s) for s in sizes))
    avcC = _box(b"avcC", bytes([1, sps[1], sps[2], sps[3], 0xFF, 0xE1]) + struct.pack(">H", len(sps)) + sps + bytes([1]) + struct.pack(">H", len(pps)) + pps)
    avc1 = _box(b"avc1", b"\x00" * 6 + struct.pack(">H", 1) + b"\x00" * 16 + struct.pack(">HH", w_px, h_px)
                + struct.pack(">II", 0x00480000, 0x00480000) + b"\x00" * 4 + struct.pack(">H", 1) + b"\x00" * 32
                + struct.pack(">Hh", 0x18, -1) + avcC)
    stsd = _full(b"stsd", 0, 0, struct.pack(">I", 1) + avc1)

    def build(chunk_off):
        stco = _full(b"stco", 0, 0, struct.pack(">II", 1, chunk_off))
        stbl = _box(b"stbl", stsd + stts + stss + stsc + stsz + stco)
        vmhd = _full(b"vmhd", 0, 1, struct.pack(">HHHH", 0, 0, 0, 0))
        dinf = _box(b"dinf", _full(b"dref", 0, 0, struct.pack(">I", 1) + _full(b"url ", 0, 1, b"")))
        minf = _box(b"minf", vmhd + dinf + stbl)
        mdhd = _full(b"mdhd", 0, 0, struct.pack(">IIIIHH", 0, 0, TIMESCALE, total, 0x55C4, 0))
        hdlr = _full(b"hdlr", 0, 0, struct.pack(">I", 0) + b"vide" + b"\x00" * 12 + b"VideoHandler\x00")
        mdia = _box(b"mdia", mdhd + hdlr + minf)
        tkhd = _full(b"tkhd", 0, 7, struct.pack(">IIII", 0, 0, 1, 0) + struct.pack(">I", total) + b"\x00" * 8
                     + struct.pack(">hhhh", 0, 0, 0, 0)
                     + struct.pack(">9i", 0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x40000000)
                     + struct.pack(">II", w_px << 16, h_px << 16))
        trak = _box(b"trak", tkhd + mdia)
        mvhd = _full(b"mvhd", 0, 0, struct.pack(">IIII", 0, 0, TIMESCALE, total) + struct.pack(">IH", 0x00010000, 0x0100)
                     + b"\x00" * 10 + struct.pack(">9i", 0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x40000000)
                     + b"\x00" * 24 + struct.pack(">I", 2))
        return _box(b"moov", mvhd + trak)

    moov0 = build(0)
    moov = build(len(ftyp) + len(moov0) + 8)
    with open(out_path, "wb") as f:
        f.write(ftyp + moov + _box(b"mdat", bytes(mdat)))
    return total / TIMESCALE


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)
    path = sys.argv[1]
    outdir = sys.argv[2] if len(sys.argv) > 2 else "."
    os.makedirs(outdir, exist_ok=True)
    r = parse_vdf(open(path, "rb").read())
    print(f"vehicle={r['vehicle']} driver={r['driver']}")

    for ch, name in ((0, "front"), (1, "rear")):
        if not r["frames"].get(ch):
            continue
        dur = write_mp4(r["frames"][ch], os.path.join(outdir, f"{name}.mp4"))
        f0 = r["frames"][ch][0]
        print(f"{name}.mp4: {len(r['frames'][ch])} frames {f0[3]}x{f0[4]} {dur:.2f}s")

    with open(os.path.join(outdir, "speed_rpm.csv"), "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["datetime", "speed_kmh", "rpm"])
        for ts, sub, spd, rpm in r["speed_rpm"]:
            w.writerow([fmt_ts(ts, sub), spd / 100, rpm])
    with open(os.path.join(outdir, "g_sensor.csv"), "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["datetime", "g_front_back", "g_left_right", "g_up_down"])
        for ts, sub, x, y, z in r["g"]:
            w.writerow([fmt_ts(ts, sub), x / 1000, y / 1000, z / 1000])
    with open(os.path.join(outdir, "gps.csv"), "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["datetime", "fix", "lat", "lon", "heading_deg"])
        for ts, sub, fix, lat, lon, heading in r["gps"]:
            w.writerow([fmt_ts(ts, sub), fix, lat, lon, heading])
    with open(os.path.join(outdir, "events.csv"), "w", newline="") as f:
        w = csv.writer(f)
        w.writerow(["datetime", "event_code"])
        for ts, sub, code in r["events"]:
            w.writerow([fmt_ts(ts, sub), code])
    print(f"telemetry: G={len(r['g'])} speed/rpm={len(r['speed_rpm'])} "
          f"gps={len(r['gps'])} events={len(r['events'])}")


if __name__ == "__main__":
    main()
