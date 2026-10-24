#include "CEncoder.h"

#include "alvr_server/Utils.h"
#include <algorithm>
#include <chrono>
#include <thread>
CEncoder::CEncoder()
    : m_bExiting(false)
    , m_targetTimestampNs(0) {
    m_encodeFinished.Set();
}

CEncoder::~CEncoder() {
    if (m_videoEncoder) {
        m_videoEncoder->Shutdown();
        m_videoEncoder.reset();
    }
}

void CEncoder::Initialize(std::shared_ptr<CD3DRender> d3dRender) {
    m_FrameRender = std::make_shared<FrameRender>(d3dRender);
    m_FrameRender->Startup();

    if (Settings::Instance().m_castMode) {
        m_desktopCapture = std::make_unique<DesktopCapture>(d3dRender.get());
        if (!m_desktopCapture->Initialize(Settings::Instance().m_castMonitorIndex)) {
            m_desktopCapture.reset();
            Error("DesktopCast: unavailable, streaming the normal VR picture.\n");
        } else {
            Info("DesktopCast: streaming the desktop to the headset.\n");
        }
    }

    uint32_t encoderWidth, encoderHeight;

    m_FrameRender->GetEncodingResolution(&encoderWidth, &encoderHeight);

    Exception vceException;
    Exception nvencException;
#ifdef ALVR_GPL
    Exception swException;
    if (Settings::Instance().m_force_sw_encoding) {
        try {
            Debug("Try to use VideoEncoderSW.\n");
            m_videoEncoder
                = std::make_shared<VideoEncoderSW>(d3dRender, encoderWidth, encoderHeight);
            m_videoEncoder->Initialize();
            return;
        } catch (Exception e) {
            swException = e;
        }
    }
#endif

    try {
        Debug("Try to use VideoEncoderAMF.\n");
        m_videoEncoder = std::make_shared<VideoEncoderAMF>(d3dRender, encoderWidth, encoderHeight);
        m_videoEncoder->Initialize();
        return;
    } catch (Exception e) {
        vceException = e;
    }
    try {
        Debug("Try to use VideoEncoderNVENC.\n");
        m_videoEncoder
            = std::make_shared<VideoEncoderNVENC>(d3dRender, encoderWidth, encoderHeight);
        m_videoEncoder->Initialize();
        return;
    } catch (Exception e) {
        nvencException = e;
    }
#ifdef ALVR_GPL
    try {
        Debug("Try to use VideoEncoderSW.\n");
        m_videoEncoder = std::make_shared<VideoEncoderSW>(d3dRender, encoderWidth, encoderHeight);
        m_videoEncoder->Initialize();
        return;
    } catch (Exception e) {
        swException = e;
    }
    throw MakeException(
        "All VideoEncoder are not available. VCE: %s, NVENC: %s, SW: %s",
        vceException.what(),
        nvencException.what(),
        swException.what()
    );
#else
    throw MakeException(
        "All VideoEncoder are not available. VCE: %s, NVENC: %s",
        vceException.what(),
        nvencException.what()
    );
#endif
}

void CEncoder::SetViewsConfig(
    vr::HmdRect2_t projLeft,
    vr::HmdMatrix34_t eyeToHeadLeft,
    vr::HmdRect2_t projRight,
    vr::HmdMatrix34_t eyeToHeadRight
) {
    m_FrameRender->SetViewsConfig(projLeft, eyeToHeadLeft, projRight, eyeToHeadRight);
}

bool CEncoder::CopyToStaging(
    ID3D11Texture2D* pTexture[][2],
    vr::VRTextureBounds_t bounds[][2],
    vr::HmdMatrix34_t poses[],
    int layerCount,
    bool recentering,
    uint64_t presentationTime,
    uint64_t targetTimestampNs,
    const std::string& message,
    const std::string& debugText
) {
    m_presentationTime = presentationTime;
    m_targetTimestampNs = targetTimestampNs;
    m_FrameRender->Startup();

    m_FrameRender->RenderFrame(
        pTexture, bounds, poses, layerCount, recentering, message, debugText
    );
    return true;
}

bool CEncoder::RenderCastFrame() {
    if (!m_desktopCapture) {
        return false;
    }

    m_desktopCapture->AcquireFrame(
        Settings::Instance().m_castScreenSize,
        (int32_t)Settings::Instance().m_castStereoSeparationPx
    );

    ID3D11Texture2D* eyeTextures[2] = { nullptr, nullptr };
    for (int eye = 0; eye < 2; eye++) {
        if (!m_desktopCapture->GetCanvas(eye, &eyeTextures[eye])) {
            return false;
        }
    }

    // One layer holding the desktop canvas for both eyes, no re-centering needed.
    ID3D11Texture2D* layers[1][2] = { { eyeTextures[0], eyeTextures[1] } };
    vr::VRTextureBounds_t bounds[1][2] = {
        { { 0.0f, 0.0f, 1.0f, 1.0f }, { 0.0f, 0.0f, 1.0f, 1.0f } },
    };
    vr::HmdMatrix34_t pose = {};
    pose.m[0][0] = 1.0f;
    pose.m[1][1] = 1.0f;
    pose.m[2][2] = 1.0f;
    vr::HmdMatrix34_t poses[1] = { pose };

    m_FrameRender->Startup();
    m_FrameRender->RenderFrame(layers, bounds, poses, 1, false, "", "");

    m_presentationTime = GetTimestampUs() * 1000;
    m_targetTimestampNs = GetTimestampUs() * 1000;

    return true;
}

void CEncoder::Run() {
    Debug("CEncoder: Start thread. Id=%d\n", GetCurrentThreadId());
    SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_MOST_URGENT);

    // In cast mode nothing calls Present() unless a VR app happens to be running, so the loop
    // drives itself at the configured capture rate and feeds the encoder directly.
    const uint32_t castFps
        = std::max<uint32_t>(1, std::min<uint32_t>(Settings::Instance().m_castCaptureFps, 240));
    const auto castInterval = std::chrono::milliseconds(1000 / castFps);

    while (!m_bExiting) {
        if (m_desktopCapture) {
            const auto started = std::chrono::steady_clock::now();

            if (RenderCastFrame()) {
                if (m_FrameRender->GetTexture()) {
                    m_videoEncoder->Transmit(
                        m_FrameRender->GetTexture().Get(),
                        m_presentationTime,
                        m_targetTimestampNs,
                        m_scheduler.CheckIDRInsertion()
                    );
                }
            }

            m_encodeFinished.Set();

            const auto remaining = castInterval - (std::chrono::steady_clock::now() - started);
            if (remaining > decltype(remaining)::zero()) {
                std::this_thread::sleep_for(remaining);
            }
            continue;
        }

        m_newFrameReady.Wait();
        if (m_bExiting)
            break;

        if (m_FrameRender->GetTexture()) {
            m_videoEncoder->Transmit(
                m_FrameRender->GetTexture().Get(),
                m_presentationTime,
                m_targetTimestampNs,
                m_scheduler.CheckIDRInsertion()
            );
        }

        m_encodeFinished.Set();
    }
}

void CEncoder::Stop() {
    m_bExiting = true;
    m_newFrameReady.Set();
    Join();
    if (m_desktopCapture) {
        m_desktopCapture->Stop();
        m_desktopCapture.reset();
    }
    m_FrameRender.reset();
}

void CEncoder::NewFrameReady() {
    m_encodeFinished.Reset();
    m_newFrameReady.Set();
}

void CEncoder::WaitForEncode() { m_encodeFinished.Wait(); }

void CEncoder::OnStreamStart() { m_scheduler.OnStreamStart(); }

void CEncoder::OnPacketLoss() { m_scheduler.OnPacketLoss(); }

void CEncoder::InsertIDR() { m_scheduler.InsertIDR(); }

void CEncoder::CaptureFrame() { }
