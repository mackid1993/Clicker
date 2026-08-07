/*
 * rd_media — a flat C surface over FFmpeg for RustDVR.
 *
 * This exists so that Rust never has to know the layout of an AVFrame. Binding
 * FFmpeg's structs directly means either bindgen (which drags in libclang) or
 * hand-transcribed struct definitions, where one wrong field offset is silent
 * memory corruption rather than a compile error. Everything here is opaque
 * pointers and scalars, so the ABI is only functions.
 *
 * It is deliberately not a player. There is no threading, no clock and no
 * synchronization in this file: one call to rd_next() advances the pipeline by
 * one decoded frame, and Rust decides what that means. The interesting logic
 * belongs in the language that can be trusted with it.
 *
 * An AVFormatContext is not thread safe, so the entire demux-and-decode pump
 * lives on whichever single thread Rust chooses to call it from.
 */

#include <libavformat/avformat.h>
#include <libavcodec/avcodec.h>
#include <libavutil/channel_layout.h>
#include <libavutil/cpu.h>
#include <libavutil/error.h>
#include <libavutil/imgutils.h>
#include <libavutil/opt.h>
#include <libavutil/time.h>
#include <libswscale/swscale.h>
#include <libswresample/swresample.h>

#include <inttypes.h>
#include <string.h>

#define RD_NOTHING   0
#define RD_VIDEO     1
#define RD_AUDIO     2
#define RD_EOF      -1
#define RD_ERROR    -2

struct RdMedia {
    AVFormatContext *fmt;

    /* Set by another thread to make a blocking read give up.
     *
     * Owned by the caller, not by this struct, and guaranteed by it to outlive
     * rd_close: FFmpeg keeps the pointer inside the format context for as long
     * as that context exists. Without this there is no way to cancel an
     * in-flight av_read_frame, and tearing a player down had to wait for the
     * network — up to the 15s rw_timeout below — with the UI thread blocked in
     * the join the whole time. */
    volatile int *abort_flag;

    int video_index;
    int audio_index;
    AVCodecContext *video_dec;
    AVCodecContext *audio_dec;

    AVPacket *packet;
    AVFrame  *frame;

    struct SwsContext *sws;
    int sws_width, sws_height, sws_format;

    SwrContext *swr;
    int out_rate;
    int out_channels;

    /* Which decoder the packet currently in flight belongs to, so rd_next can
     * keep draining frames from it before reading another packet. A decoder
     * can emit several frames from one packet and dropping them would show up
     * as stutter that looks like a decode fault. */
    int draining;

    double video_timebase;
    double audio_timebase;

    /* The last frame decoded, kept until the caller copies it out. */
    int have_video;
    int have_audio;
    int audio_samples;

    /* Closed captions.
     *
     * Broadcast captions are not a stream. CEA-608 and CEA-708 ride inside the
     * video itself — in H.264 SEI user-data, in MPEG-2 picture user-data — and
     * FFmpeg hands them out as A53 side data attached to each decoded video
     * frame. So there is nothing to select with av_find_best_stream and
     * nothing in nb_streams to find: captions exist only once pictures are
     * being decoded, which is why nothing here saw them before.
     *
     * The bytes are fed to FFmpeg's own EIA-608 decoder, which is what turns
     * control codes and roll-up positioning into lines of text. */
    AVCodecContext *cc_dec;
    /* Set once A53 data has actually been seen, so the interface can offer
     * captions only where they exist rather than always. */
    int cc_seen;
    int cc_enabled;
    /* The most recent caption line, waiting to be collected. */
    char cc_text[512];
    int cc_ready;

    /* Microseconds spent inside av_read_frame and inside the decoders since
     * the last time the caller read them. rd_next is one call from Rust's
     * point of view, but it contains two entirely different kinds of work —
     * a blocking HTTP segment fetch and CPU-bound decoding — and a single
     * timing figure covering both cannot say which one is the problem. */
    int64_t read_us;
    int64_t decode_us;
    int64_t read_max_us;
};

typedef struct RdMedia RdMedia;

/* FFmpeg polls this throughout blocking I/O. Returning nonzero makes the
 * current operation fail with AVERROR_EXIT instead of running to completion. */
static int rd_aborted(void *opaque)
{
    const volatile int *flag = (const volatile int *)opaque;
    return flag && *flag;
}

static void set_err(char *err, int errlen, const char *msg, int code)
{
    if (!err || errlen <= 0) return;
    char buf[AV_ERROR_MAX_STRING_SIZE] = {0};
    if (code != 0) {
        av_strerror(code, buf, sizeof(buf));
        snprintf(err, errlen, "%s: %s", msg, buf);
    } else {
        snprintf(err, errlen, "%s", msg);
    }
}

static AVCodecContext *open_decoder(AVFormatContext *fmt, int index, char *err, int errlen)
{
    AVStream *stream = fmt->streams[index];
    const AVCodec *codec = avcodec_find_decoder(stream->codecpar->codec_id);
    if (!codec) {
        set_err(err, errlen, "no decoder for stream", 0);
        return NULL;
    }

    AVCodecContext *ctx = avcodec_alloc_context3(codec);
    if (!ctx) return NULL;

    int rc = avcodec_parameters_to_context(ctx, stream->codecpar);
    if (rc < 0) {
        set_err(err, errlen, "avcodec_parameters_to_context", rc);
        avcodec_free_context(&ctx);
        return NULL;
    }

    /*
     * Slice threading only, and a hard cap on the count.
     *
     * "thread_count = 0" means one thread per core, which on a 22 core machine
     * is 22. With FF_THREAD_FRAME that is 22 complete decoding contexts, each
     * with its own picture buffers: hundreds of megabytes of resident memory
     * for a single 1080p stream, and 22 frames of added latency, on a live
     * stream where latency is the thing being minimized.
     *
     * Slice threading splits one frame across cores instead of decoding
     * several frames at once. It uses a fraction of the memory, adds no
     * latency at all, and six threads is already far more than 1080p60 needs.
     */
    int cores = av_cpu_count();
    ctx->thread_count = cores > 6 ? 6 : (cores > 0 ? cores : 1);
    ctx->thread_type = FF_THREAD_SLICE;
    ctx->pkt_timebase = stream->time_base;

    rc = avcodec_open2(ctx, codec, NULL);
    if (rc < 0) {
        set_err(err, errlen, "avcodec_open2", rc);
        avcodec_free_context(&ctx);
        return NULL;
    }
    return ctx;
}

RdMedia *rd_open(const char *url, int out_rate, int out_channels,
                 int live_start_index, volatile int *abort_flag,
                 char *err, int errlen)
{
    /* Warnings and errors only. The default level narrates every segment
     * fetch, which on a live HLS stream is a line of output every couple of
     * seconds, drowning out anything worth reading. */
    av_log_set_level(AV_LOG_WARNING);

    RdMedia *m = av_mallocz(sizeof(RdMedia));
    if (!m) return NULL;

    m->video_index = -1;
    m->audio_index = -1;
    m->out_rate = out_rate > 0 ? out_rate : 48000;
    m->out_channels = out_channels > 0 ? out_channels : 2;
    m->abort_flag = abort_flag;

    /* The context is allocated here rather than by avformat_open_input so the
     * interrupt callback is installed before the first byte is fetched. Opening
     * a live HLS master playlist is itself a blocking network operation, and one
     * that cannot be cancelled is one that holds up teardown just as badly as a
     * read. On failure avformat_open_input frees this and NULLs the pointer, so
     * the error paths below stay correct. */
    m->fmt = avformat_alloc_context();
    if (!m->fmt) {
        set_err(err, errlen, "avformat_alloc_context", 0);
        av_free(m);
        return NULL;
    }
    m->fmt->interrupt_callback.callback = rd_aborted;
    m->fmt->interrupt_callback.opaque = (void *)abort_flag;

    AVDictionary *opts = NULL;
    /* A live transport stream has no index and no reliable duration. Reading a
     * larger probe makes stream detection reliable on a stream that starts
     * mid-GOP, which is every live tune. */
    av_dict_set(&opts, "probesize", "8000000", 0);
    av_dict_set(&opts, "analyzeduration", "3000000", 0);
    /* Without a timeout a dead server hangs the thread forever. */
    av_dict_set(&opts, "rw_timeout", "15000000", 0);
    av_dict_set(&opts, "user_agent", "RustDVR", 0);

    /*
     * Start at the head of the playlist, not three segments from its end.
     *
     * FFmpeg's default of live_start_index=-3 puts the read position two
     * segments (four seconds) from the live edge, and avformat_find_stream_info
     * then spends its analyzeduration reading forward through them. What is
     * left is nothing: the demuxer ends up asking for the segment the server is
     * still writing, and av_read_frame blocks until it is finished. Measured on
     * this stream, single reads grew 121ms, 231ms, 343ms, 446ms across
     * successive seconds and eventually hit the 15s rw_timeout, at which point
     * the session resynchronized onto a completely different timestamp base.
     * Everything downstream saw that as stutter: the frame queue never rose
     * above one, the presentation thread found it empty on a third of its
     * wake-ups, and the audio queue sat at zero and underran continuously.
     *
     * A live player must not stand on the live edge. Channels keeps every
     * segment from the moment of tuning and never advances EXT-X-MEDIA-SEQUENCE
     * (verified against the server), so starting at the head costs the length
     * of the playlist at open — a few seconds — and buys a buffer that lives on
     * the server rather than in this process's memory. Reads then hit segments
     * that are already complete, and complete at LAN speed.
     *
     * It is also the only value that makes seeking exact. FFmpeg's HLS seek
     * builds its timeline by accumulating EXT-X-INF durations from segments[0]
     * and anchoring that sum at the DTS of the first packet it ever read
     * (hls.c, find_timestamp_in_playlist). Those two only describe the same
     * instant when the first packet read came from segments[0], which is what
     * index 0 guarantees. At -3 the whole seekable timeline is skewed by
     * however many segments happened to be in the playlist at open, which is
     * not a quantity this side can know, let alone correct for.
     *
     * That reasoning holds for a *fresh* tune, where the playlist is only a few
     * seconds long. It does not hold for re-opening a channel that has been
     * playing for a while — changing quality — because Channels' playlist still
     * carries every segment since the tune, so its head is now twenty minutes
     * back. Rejoining there sends someone watching live to whenever they first
     * tuned, and no amount of seeking afterwards can fix it: a live stream
     * states no duration, so the seekable window is measured from the
     * timestamps actually decoded and only grows at playback speed. The join
     * point has to be chosen here, which is what the caller passes in.
     */
    char start_index[16];
    snprintf(start_index, sizeof(start_index), "%d", live_start_index);
    av_dict_set(&opts, "live_start_index", start_index, 0);

    int rc = avformat_open_input(&m->fmt, url, NULL, &opts);
    av_dict_free(&opts);
    if (rc < 0) {
        set_err(err, errlen, "avformat_open_input", rc);
        av_free(m);
        return NULL;
    }

    rc = avformat_find_stream_info(m->fmt, NULL);
    if (rc < 0) {
        set_err(err, errlen, "avformat_find_stream_info", rc);
        avformat_close_input(&m->fmt);
        av_free(m);
        return NULL;
    }

    m->video_index = av_find_best_stream(m->fmt, AVMEDIA_TYPE_VIDEO, -1, -1, NULL, 0);
    m->audio_index = av_find_best_stream(m->fmt, AVMEDIA_TYPE_AUDIO, -1, -1, NULL, 0);

    if (m->video_index < 0) {
        set_err(err, errlen, "no video stream", 0);
        avformat_close_input(&m->fmt);
        av_free(m);
        return NULL;
    }

    m->video_dec = open_decoder(m->fmt, m->video_index, err, errlen);
    if (!m->video_dec) {
        avformat_close_input(&m->fmt);
        av_free(m);
        return NULL;
    }
    m->video_timebase = av_q2d(m->fmt->streams[m->video_index]->time_base);

    if (m->audio_index >= 0) {
        /* Audio failing is survivable: a picture with no sound beats no
         * picture at all, and the caller is told by rd_has_audio(). */
        m->audio_dec = open_decoder(m->fmt, m->audio_index, NULL, 0);
        if (m->audio_dec) {
            m->audio_timebase = av_q2d(m->fmt->streams[m->audio_index]->time_base);

            AVChannelLayout out_layout;
            av_channel_layout_default(&out_layout, m->out_channels);
            rc = swr_alloc_set_opts2(&m->swr,
                                     &out_layout, AV_SAMPLE_FMT_FLT, m->out_rate,
                                     &m->audio_dec->ch_layout,
                                     m->audio_dec->sample_fmt,
                                     m->audio_dec->sample_rate,
                                     0, NULL);
            av_channel_layout_uninit(&out_layout);
            if (rc < 0 || swr_init(m->swr) < 0) {
                swr_free(&m->swr);
                avcodec_free_context(&m->audio_dec);
                m->audio_index = -1;
            }
        } else {
            m->audio_index = -1;
        }
    }

    /*
     * Ask the server for one rendition, not five.
     *
     * A Channels master playlist offers five variants of the same program at
     * every requested resolution (measured: resolution=360, 720 and 1080 each
     * return five #EXT-X-STREAM-INF entries). FFmpeg's HLS demuxer considers
     * every playlist whose streams are not discarded to be needed and fetches
     * them all concurrently, so watching one stream started five server-side
     * transcodes of the same file. The four nobody sees compete with the one
     * being watched for the same encoder, and the picture stutters — worst
     * immediately after a quality change, when five more begin before the
     * previous five have finished being torn down.
     *
     * hls.c recomputes playlist->needed from the discard flags of the streams
     * it feeds (playlist_needed, called from recheck_discard_flags), so marking
     * everything except the two chosen streams AVDISCARD_ALL stops the other
     * playlists from being downloaded at all.
     */
    int discarded = 0;
    for (unsigned i = 0; i < m->fmt->nb_streams; i++) {
        if ((int)i != m->video_index && (int)i != m->audio_index) {
            m->fmt->streams[i]->discard = AVDISCARD_ALL;
            discarded++;
        }
    }
    if (discarded > 0) {
        av_log(NULL, AV_LOG_WARNING,
               "rd_media: using streams %d/%d, discarded %d others\n",
               m->video_index, m->audio_index, discarded);
    }

    m->packet = av_packet_alloc();
    m->frame = av_frame_alloc();
    if (!m->packet || !m->frame) {
        set_err(err, errlen, "out of memory", 0);
        return NULL;
    }
    return m;
}

void rd_close(RdMedia *m)
{
    if (!m) return;
    if (m->sws) sws_freeContext(m->sws);
    if (m->swr) swr_free(&m->swr);
    if (m->cc_dec) avcodec_free_context(&m->cc_dec);
    if (m->frame) av_frame_free(&m->frame);
    if (m->packet) av_packet_free(&m->packet);
    if (m->video_dec) avcodec_free_context(&m->video_dec);
    if (m->audio_dec) avcodec_free_context(&m->audio_dec);
    if (m->fmt) avformat_close_input(&m->fmt);
    av_free(m);
}

int rd_video_width(RdMedia *m)  { return m && m->video_dec ? m->video_dec->width : 0; }
int rd_video_height(RdMedia *m) { return m && m->video_dec ? m->video_dec->height : 0; }

/* The size of the picture actually decoded, which is not always the size the
 * container advertised at open.
 *
 * The caller sizes its destination buffer once, from rd_video_width/height,
 * which come from the codec parameters the container carried. A Channels HLS
 * master playlist offers several variants and the decoder reports whatever it
 * ends up being fed; H.264 also permits the resolution to change mid-stream at
 * an IDR. When those disagree the copy below writes past the end of a buffer
 * sized for the smaller one, which is a heap corruption, not a glitch. Ask for
 * these before every copy and resize when they move. */
int rd_frame_width(RdMedia *m)  { return m && m->have_video && m->frame ? m->frame->width : 0; }
int rd_frame_height(RdMedia *m) { return m && m->have_video && m->frame ? m->frame->height : 0; }
int rd_has_audio(RdMedia *m)    { return m && m->audio_index >= 0; }
int rd_out_rate(RdMedia *m)     { return m ? m->out_rate : 0; }
int rd_out_channels(RdMedia *m) { return m ? m->out_channels : 0; }

/* Seconds, or a negative value when the container will not say — which is the
 * normal case for a live stream. */
double rd_duration(RdMedia *m)
{
    if (!m || !m->fmt) return -1.0;
    if (m->fmt->duration == AV_NOPTS_VALUE) return -1.0;
    return (double)m->fmt->duration / (double)AV_TIME_BASE;
}

int rd_seekable(RdMedia *m)
{
    if (!m || !m->fmt) return 0;

    /* A byte stream that can be repositioned: an ordinary file, or HTTP with
     * range support. */
    if (m->fmt->pb && m->fmt->pb->seekable) return 1;

    /* A known duration means the demuxer has the whole thing indexed. */
    if (m->fmt->duration != AV_NOPTS_VALUE) return 1;

    /*
     * A live HLS playlist reports no duration at all, because it has no
     * EXT-X-ENDLIST and the demuxer will not guess where it stops. That is not
     * the same as being unseekable: the demuxer holds every segment the
     * playlist has listed, and Channels never drops one, so the entire session
     * from the moment of tuning is addressable.
     *
     * Requiring a duration here is what made the app report "not seekable" on
     * a stream it could in fact rewind through perfectly well.
     */
    if (m->fmt->iformat && m->fmt->iformat->name &&
        strstr(m->fmt->iformat->name, "hls")) {
        return 1;
    }

    /* No pb at all and no duration: the demuxer owns its own I/O and has said
     * nothing useful. Assume it cannot seek rather than offering an action
     * that breaks playback. */
    return 0;
}

/* Read and reset the accumulated timings. */
void rd_take_timings(RdMedia *m, int64_t *read_us, int64_t *decode_us, int64_t *read_max_us)
{
    if (!m) return;
    if (read_us)     { *read_us = m->read_us;         m->read_us = 0; }
    if (decode_us)   { *decode_us = m->decode_us;     m->decode_us = 0; }
    if (read_max_us) { *read_max_us = m->read_max_us; m->read_max_us = 0; }
}

/* Strip an ASS dialogue line down to what it says.
 *
 * FFmpeg's caption decoder emits ASS, whose payload is the tenth
 * comma-separated field: "Dialogue: 0,0:00:01.00,0:00:03.00,Default,,0,0,0,,TEXT".
 * Everything before it is styling this player does not honour, and the
 * inline {\an7} style overrides inside it are positioning that only means
 * anything against a full ASS renderer. */
static void ass_to_text(const char *ass, char *out, int outlen)
{
    out[0] = '\0';
    if (!ass) return;

    const char *body = ass;
    int commas = 0;
    for (const char *p = ass; *p; p++) {
        if (*p == ',' && ++commas == 9) {
            body = p + 1;
            break;
        }
    }
    if (commas < 9) body = ass;

    int w = 0;
    for (const char *p = body; *p && w < outlen - 1; p++) {
        if (*p == '{') {                      /* {\an7} and friends */
            while (*p && *p != '}') p++;
            if (!*p) break;
            continue;
        }
        if (p[0] == '\\' && (p[1] == 'N' || p[1] == 'n')) {
            out[w++] = '\n';
            p++;
            continue;
        }
        if (*p == '\r') continue;
        out[w++] = *p;
    }
    out[w] = '\0';
}

/* Open the EIA-608 decoder the first time captions are actually seen. */
static void cc_open(RdMedia *m)
{
    if (m->cc_dec) return;
    const AVCodec *codec = avcodec_find_decoder(AV_CODEC_ID_EIA_608);
    if (!codec) return;
    m->cc_dec = avcodec_alloc_context3(codec);
    if (!m->cc_dec) return;
    if (avcodec_open2(m->cc_dec, codec, NULL) < 0) {
        avcodec_free_context(&m->cc_dec);
    }
}

/* Pull captions out of a decoded video frame, if it carries any. */
static void cc_from_frame(RdMedia *m)
{
    AVFrameSideData *sd = av_frame_get_side_data(m->frame, AV_FRAME_DATA_A53_CC);
    if (!sd || sd->size == 0) return;

    m->cc_seen = 1;
    if (!m->cc_enabled) return;

    cc_open(m);
    if (!m->cc_dec) return;

    AVPacket *packet = av_packet_alloc();
    if (!packet) return;
    if (av_new_packet(packet, (int)sd->size) < 0) {
        av_packet_free(&packet);
        return;
    }
    memcpy(packet->data, sd->data, sd->size);
    packet->pts = m->frame->best_effort_timestamp;

    AVSubtitle sub;
    int got = 0;
    if (avcodec_decode_subtitle2(m->cc_dec, &sub, &got, packet) >= 0 && got) {
        for (unsigned i = 0; i < sub.num_rects; i++) {
            const AVSubtitleRect *rect = sub.rects[i];
            const char *text = rect->ass ? rect->ass : rect->text;
            if (!text) continue;
            ass_to_text(text, m->cc_text, (int)sizeof(m->cc_text));
            /* An empty line is the decoder clearing the screen, which is a
             * caption in its own right: without it the last line stays up
             * over the silence that follows it. */
            m->cc_ready = 1;
        }
        avsubtitle_free(&sub);
    }
    av_packet_free(&packet);
}

int rd_cc_available(RdMedia *m) { return m && m->cc_seen; }

void rd_cc_enable(RdMedia *m, int on)
{
    if (!m) return;
    m->cc_enabled = on ? 1 : 0;
    if (!on) {
        m->cc_text[0] = '\0';
        m->cc_ready = 1;   /* so the caller clears what is on screen */
    }
}

/* Collect the latest caption line. Returns 1 when one was written. */
int rd_cc_take(RdMedia *m, char *out, int outlen)
{
    if (!m || !out || outlen <= 0 || !m->cc_ready) return 0;
    snprintf(out, outlen, "%s", m->cc_text);
    m->cc_ready = 0;
    return 1;
}

static int decode_frame_from(RdMedia *m, AVCodecContext *dec, int kind, double *pts_out)
{
    int64_t t0 = av_gettime_relative();
    int rc = avcodec_receive_frame(dec, m->frame);
    m->decode_us += av_gettime_relative() - t0;
    if (rc == AVERROR(EAGAIN) || rc == AVERROR_EOF) return RD_NOTHING;
    if (rc < 0) return RD_ERROR;

    int64_t pts = m->frame->best_effort_timestamp;
    if (pts == AV_NOPTS_VALUE) pts = m->frame->pts;

    double tb = (kind == RD_VIDEO) ? m->video_timebase : m->audio_timebase;
    *pts_out = (pts == AV_NOPTS_VALUE) ? -1.0 : (double)pts * tb;

    if (kind == RD_VIDEO) {
        m->have_video = 1;
        cc_from_frame(m);
    } else {
        m->have_audio = 1;
        /* How many output samples this will become once resampled, including
         * anything the resampler is still holding. */
        int64_t delay = swr_get_delay(m->swr, m->audio_dec->sample_rate);
        m->audio_samples = (int)av_rescale_rnd(delay + m->frame->nb_samples,
                                               m->out_rate,
                                               m->audio_dec->sample_rate,
                                               AV_ROUND_UP);
    }
    return kind;
}

/*
 * Advance by one decoded frame.
 *
 * Returns RD_VIDEO or RD_AUDIO with *pts_out set, RD_NOTHING if it needs to be
 * called again, RD_EOF at the end, or RD_ERROR. Frames are drained from a
 * decoder before the next packet is read, because one packet can produce
 * several frames and discarding them reads as stutter.
 */
int rd_next(RdMedia *m, double *pts_out)
{
    if (!m) return RD_ERROR;
    av_frame_unref(m->frame);
    m->have_video = m->have_audio = 0;

    if (m->draining == RD_VIDEO) {
        int r = decode_frame_from(m, m->video_dec, RD_VIDEO, pts_out);
        if (r != RD_NOTHING) return r;
        m->draining = 0;
    } else if (m->draining == RD_AUDIO) {
        int r = decode_frame_from(m, m->audio_dec, RD_AUDIO, pts_out);
        if (r != RD_NOTHING) return r;
        m->draining = 0;
    }

    av_packet_unref(m->packet);
    int64_t t0 = av_gettime_relative();
    int rc = av_read_frame(m->fmt, m->packet);
    int64_t took = av_gettime_relative() - t0;
    m->read_us += took;
    if (took > m->read_max_us) m->read_max_us = took;
    if (rc == AVERROR_EOF) return RD_EOF;
    if (rc < 0) return RD_ERROR;

    if (m->packet->stream_index == m->video_index) {
        int64_t t1 = av_gettime_relative();
        int sent = avcodec_send_packet(m->video_dec, m->packet);
        m->decode_us += av_gettime_relative() - t1;
        if (sent < 0) return RD_NOTHING;
        m->draining = RD_VIDEO;
        int r = decode_frame_from(m, m->video_dec, RD_VIDEO, pts_out);
        if (r == RD_NOTHING) m->draining = 0;
        return r;
    }

    if (m->audio_index >= 0 && m->packet->stream_index == m->audio_index) {
        if (avcodec_send_packet(m->audio_dec, m->packet) < 0) return RD_NOTHING;
        m->draining = RD_AUDIO;
        int r = decode_frame_from(m, m->audio_dec, RD_AUDIO, pts_out);
        if (r == RD_NOTHING) m->draining = 0;
        return r;
    }

    return RD_NOTHING;
}

/* Copy the last decoded picture out as RGBA. */
int rd_video_copy(RdMedia *m, uint8_t *dst, int dst_stride,
                  int dst_width, int dst_height)
{
    if (!m || !m->have_video || !dst) return -1;

    /* Refuse rather than corrupt.
     *
     * sws_scale writes m->frame->height rows of m->frame->width pixels into
     * dst. Nothing here has ever checked that against the buffer the caller
     * allocated, which is sized once at open from the container's idea of the
     * picture size. Switching stream quality is exactly when the two stop
     * agreeing, and a 1080p frame written into a buffer sized for 360p is a
     * 5MB overrun of the heap — which is the hard crash on changing
     * resolution, not a decode fault.
     *
     * Returning -1 makes the caller resize and ask again, and means that even
     * if it does not, the failure is a black frame rather than memory
     * corruption. */
    if (dst_width < m->frame->width || dst_height < m->frame->height ||
        dst_stride < m->frame->width * 4) {
        av_log(NULL, AV_LOG_WARNING,
               "rd_media: refusing to copy %dx%d into a %dx%d buffer (stride %d)\n",
               m->frame->width, m->frame->height, dst_width, dst_height, dst_stride);
        return -1;
    }

    if (!m->sws || m->sws_width != m->frame->width ||
        m->sws_height != m->frame->height || m->sws_format != m->frame->format) {
        if (m->sws) sws_freeContext(m->sws);

        /*
         * Built by hand rather than with sws_getContext, purely to be able to
         * set "threads" before initialization. sws_getContext has no way to
         * pass it, and the difference is not marginal: converting 1080p YUV to
         * RGBA is on the order of ten milliseconds on one core, against a
         * frame budget of 16.7ms at 60fps, on the same thread that also has to
         * demux and decode. Slicing it across cores is what makes that fit.
         *
         * SWS_POINT rather than SWS_BILINEAR because there is no scaling
         * happening here, only a color space conversion: the destination is
         * the same size as the source, so interpolation costs time and changes
         * nothing.
         */
        m->sws = sws_alloc_context();
        if (!m->sws) return -1;

        /* av_opt_set_int fails silently on a name the object does not have, so
         * a typo here would produce a context that quietly ignored the setting
         * and a conversion several times slower than intended, with nothing to
         * show for it. Every one is checked, and the count is read back below
         * so a build can be asked whether the settings actually took. */
        int bad = 0;
        bad |= av_opt_set_int(m->sws, "srcw", m->frame->width, 0) < 0;
        bad |= av_opt_set_int(m->sws, "srch", m->frame->height, 0) < 0;
        bad |= av_opt_set_int(m->sws, "src_format", m->frame->format, 0) < 0;
        bad |= av_opt_set_int(m->sws, "dstw", m->frame->width, 0) < 0;
        bad |= av_opt_set_int(m->sws, "dsth", m->frame->height, 0) < 0;
        bad |= av_opt_set_int(m->sws, "dst_format", AV_PIX_FMT_RGBA, 0) < 0;
        bad |= av_opt_set_int(m->sws, "sws_flags", SWS_POINT, 0) < 0;
        bad |= av_opt_set_int(m->sws, "threads", 0, 0) < 0;  /* 0 = one per core */
        if (bad) {
            av_log(NULL, AV_LOG_ERROR, "rd_media: swscale rejected a setting\n");
        }

        int64_t applied_threads = 0, applied_flags = 0;
        av_opt_get_int(m->sws, "threads", 0, &applied_threads);
        av_opt_get_int(m->sws, "sws_flags", 0, &applied_flags);
        av_log(NULL, AV_LOG_INFO,
               "rd_media: swscale %dx%d fmt %d -> RGBA, threads=%" PRId64
               " flags=%" PRId64 "\n",
               m->frame->width, m->frame->height, m->frame->format,
               applied_threads, applied_flags);

        if (sws_init_context(m->sws, NULL, NULL) < 0) {
            sws_freeContext(m->sws);
            m->sws = NULL;
            return -1;
        }

        m->sws_width = m->frame->width;
        m->sws_height = m->frame->height;
        m->sws_format = m->frame->format;
    }

    uint8_t *planes[4] = { dst, NULL, NULL, NULL };
    int strides[4] = { dst_stride, 0, 0, 0 };
    return sws_scale(m->sws, (const uint8_t *const *)m->frame->data,
                     m->frame->linesize, 0, m->frame->height, planes, strides);
}

/* Upper bound on the samples per channel the last audio frame will produce. */
int rd_audio_samples(RdMedia *m)
{
    return (m && m->have_audio) ? m->audio_samples : 0;
}

/* Copy the last decoded audio out as interleaved float, returning the number of
 * samples per channel actually written. */
int rd_audio_copy(RdMedia *m, float *dst, int max_samples)
{
    if (!m || !m->have_audio || !m->swr || !dst) return 0;
    uint8_t *out[1] = { (uint8_t *)dst };
    int got = swr_convert(m->swr, out, max_samples,
                          (const uint8_t **)m->frame->extended_data,
                          m->frame->nb_samples);
    return got < 0 ? 0 : got;
}

/*
 * Seek to an absolute position in seconds.
 *
 * AVSEEK_FLAG_BACKWARD lands on the keyframe at or before the target, which is
 * the only way to get a decodable picture: starting mid-GOP produces macro
 * blocking until the next keyframe arrives. The decoders are flushed
 * afterwards so no frame from before the seek is presented after it, which is
 * what makes audio and video come back in step rather than drifting.
 */
int rd_seek(RdMedia *m, double seconds)
{
    if (!m || !m->fmt) return -1;
    if (seconds < 0) seconds = 0;

    /*
     * Seek on the video stream by name rather than passing -1.
     *
     * With -1 the timestamp is in AV_TIME_BASE units and FFmpeg rescales it
     * into whichever stream av_find_default_stream_index happens to pick. That
     * is normally the video stream, but "normally" is doing a lot of work in a
     * path whose failure mode is landing on the wrong second of a broadcast.
     * Naming the stream means the timestamp is in that stream's own time base,
     * which is the same base the PTS values handed back by rd_next are in, so
     * a caller can seek to a position it was told about without a conversion
     * in between that could be wrong.
     */
    AVStream *vs = m->fmt->streams[m->video_index];
    int64_t target = (int64_t)(seconds / av_q2d(vs->time_base));

    /*
     * Hide the container's stated duration for the length of the seek.
     *
     * A live HLS playlist has no duration: it carries no EXT-X-ENDLIST, and
     * hls_read_header leaves AVFormatContext.duration unset for exactly that
     * reason. avformat_find_stream_info then fills it in anyway, by estimating
     * from the packets it happened to probe at startup — a handful of seconds,
     * describing nothing but how much was read before playback began.
     *
     * hls_read_seek believes that estimate:
     *
     *     duration = s->duration == AV_NOPTS_VALUE ? 0 : s->duration;
     *     if (0 < duration && duration < seek_timestamp - first_timestamp)
     *         return AVERROR(EIO);
     *
     * so every seek beyond that accidental number is rejected as out of range.
     * Measured on this stream: after 45 seconds of playback, skip-back asked
     * for 30.228s and got "avformat_seek_file failed: Operation not permitted",
     * while the segments covering that moment were sitting on the server,
     * listed in the playlist, and perfectly fetchable. Playback did not move.
     *
     * The estimate is put back immediately, so nothing else ever observes a
     * different stream than it did before. On a real recording the duration is
     * genuine, and removing it here costs nothing: the range it describes is
     * one this side already enforces, against the window it hands the caller.
     */
    int64_t stated_duration = m->fmt->duration;
    m->fmt->duration = AV_NOPTS_VALUE;

    /*
     * Clear AVFMTCTX_UNSEEKABLE for the length of the seek.
     *
     * This is the reason skip-back did nothing at all. FFmpeg's HLS demuxer
     * decides, on every playlist reload (hls.c, end of parse_playlist), that a
     * playlist is seekable only if it is finished or declares itself an event:
     *
     *     if (!(playlists[0]->finished || playlists[0]->type == PLS_TYPE_EVENT))
     *         c->ctx->ctx_flags |= AVFMTCTX_UNSEEKABLE;
     *
     * and hls_read_seek returns ENOSYS on that flag before it looks at
     * anything else. Channels' playlist carries no EXT-X-ENDLIST, because it
     * is live, and no EXT-X-PLAYLIST-TYPE at all, so it fails both tests and
     * every seek was rejected without a single segment being considered.
     *
     * The flag is a proxy for a real question — "might the segment I want have
     * been dropped off the front?" — and for this server the answer is no. It
     * is an event playlist in everything but the declaration:
     * EXT-X-MEDIA-SEQUENCE stays at 1 for the life of the session while the
     * list only grows, which was checked against the server directly. Nothing
     * is ever evicted, so every second since the channel was tuned is still a
     * fetchable file.
     *
     * If that assumption were ever wrong, the seek does not become unsafe: the
     * segment simply is not in the playlist, find_timestamp_in_playlist fails
     * to place the timestamp, and this returns -1 exactly as it does now. The
     * flag is restored immediately either way, so the demuxer's own view of
     * itself is unchanged and the next playlist reload re-asserts it anyway.
     */
    int stated_flags = m->fmt->ctx_flags;
    m->fmt->ctx_flags &= ~AVFMTCTX_UNSEEKABLE;

    /*
     * max_ts is the target rather than INT64_MAX on purpose. avformat_seek_file
     * strips AVSEEK_FLAG_BACKWARD and re-derives the direction from how the
     * target sits between min_ts and max_ts, and on failure it retries at
     * whichever bound the direction points to. With INT64_MAX as the bound that
     * retry asks to seek to the end of time. Pinning max_ts to the target keeps
     * the direction backwards and removes the retry entirely.
     */
    int rc = avformat_seek_file(m->fmt, m->video_index, INT64_MIN, target,
                                target, AVSEEK_FLAG_BACKWARD);
    if (rc < 0) {
        char buf[AV_ERROR_MAX_STRING_SIZE] = {0};
        av_strerror(rc, buf, sizeof(buf));
        av_log(NULL, AV_LOG_WARNING,
               "rd_media: avformat_seek_file(%.3fs) failed: %s\n", seconds, buf);
        rc = av_seek_frame(m->fmt, m->video_index, target, AVSEEK_FLAG_BACKWARD);
        if (rc < 0) {
            av_strerror(rc, buf, sizeof(buf));
            av_log(NULL, AV_LOG_WARNING,
                   "rd_media: av_seek_frame(%.3fs) failed: %s\n", seconds, buf);
            m->fmt->duration = stated_duration;
            m->fmt->ctx_flags = stated_flags;
            return -1;
        }
    }
    m->fmt->duration = stated_duration;
    m->fmt->ctx_flags = stated_flags;

    if (m->video_dec) avcodec_flush_buffers(m->video_dec);
    if (m->audio_dec) avcodec_flush_buffers(m->audio_dec);
    m->draining = 0;
    m->have_video = m->have_audio = 0;
    av_packet_unref(m->packet);
    av_frame_unref(m->frame);
    return 0;
}

/* The build's own licence string, so the shipped binary can be asked what it
 * is rather than trusted. A build carrying GPL components would say so here. */
const char *rd_license(void)
{
    return avutil_license();
}

const char *rd_version(void)
{
    return av_version_info();
}
