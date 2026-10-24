#include "DesktopCapture.h"

#include "alvr_server/Settings.h"
#include "alvr_server/Logger.h"
#include "shared/d3drender.h"
#include <algorithm>
#include <cmath>

DesktopCapture::DesktopCapture(CD3DRender* d3dRender) { m_d3dRender = d3dRender; }

void DesktopCapture::Stop() {
    m_duplication.Reset();
    m_desktopTexture.Reset();
    for (int i = 0; i < 2; i++) {
        m_canvasRtv[i].Reset();
        m_canvasTextures[i].Reset();
    }
    m_haveFrame = false;
}

bool DesktopCapture::Initialize(uint32_t monitorIndex) {
    ComPtr<IDXGIDevice> dxgiDevice;
    if (FAILED(m_d3dRender->GetDevice()->QueryInterface(IID_PPV_ARGS(&dxgiDevice)))) {
        Error("DesktopCast: failed to query the DXGI device.\n");
        return false;
    }

    ComPtr<IDXGIAdapter> adapter;
    if (FAILED(dxgiDevice->GetAdapter(&adapter))) {
        Error("DesktopCast: failed to query the DXGI adapter.\n");
        return false;
    }

    ComPtr<IDXGIOutput> output;
    if (FAILED(adapter->EnumOutputs(monitorIndex, &output))) {
        Error("DesktopCast: there is no monitor at index %u.\n", monitorIndex);
        return false;
    }

    DXGI_OUTPUT_DESC outputDesc = {};
    output->GetDesc(&outputDesc);
    Info(
        "DesktopCast: capturing monitor %u, %ldx%ld, attached=%d\n",
        monitorIndex,
        outputDesc.DesktopCoordinates.right - outputDesc.DesktopCoordinates.left,
        outputDesc.DesktopCoordinates.bottom - outputDesc.DesktopCoordinates.top,
        (int)outputDesc.AttachedToDesktop
    );

    ComPtr<IDXGIOutput1> output1;
    if (FAILED(output->QueryInterface(IID_PPV_ARGS(&output1)))) {
        Error("DesktopCast: IDXGIOutput1 is not supported.\n");
        return false;
    }

    if (FAILED(output1->DuplicateOutput(m_d3dRender->GetDevice(), &m_duplication))) {
        Error(
            "DesktopCast: DuplicateOutput failed. Desktop duplication only allows one client, "
            "close remote desktop or capture tools and restart SteamVR.\n"
        );
        return false;
    }

    Info("DesktopCast: desktop duplication is ready.\n");
    return true;
}

bool DesktopCapture::PrepareCanvases(ID3D11Texture2D* desktopTexture) {
    D3D11_TEXTURE2D_DESC desktopDesc = {};
    desktopTexture->GetDesc(&desktopDesc);

    CD3D11_TEXTURE2D_DESC canvasDesc(desktopDesc);
    canvasDesc.Width = m_canvasWidth;
    canvasDesc.Height = m_canvasHeight;
    canvasDesc.BindFlags = D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_RENDER_TARGET;
    canvasDesc.MiscFlags = 0;
    canvasDesc.SampleDesc.Count = 1;
    canvasDesc.SampleDesc.Quality = 0;

    for (int i = 0; i < 2; i++) {
        m_canvasRtv[i].Reset();
        m_canvasTextures[i].Reset();

        ComPtr<ID3D11Texture2D> canvas;
        if (FAILED(m_d3dRender->GetDevice()->CreateTexture2D(&canvasDesc, nullptr, &canvas))) {
            Error("DesktopCast: failed to create the %ux%u canvas.\n", m_canvasWidth, m_canvasHeight);
            return false;
        }
        if (FAILED(
                m_d3dRender->GetDevice()->CreateRenderTargetView(canvas.Get(), nullptr, &m_canvasRtv[i])
            )) {
            Error("DesktopCast: failed to create the canvas render target.\n");
            return false;
        }
        m_canvasTextures[i] = canvas;
    }

    m_desktopWidth = desktopDesc.Width;
    m_desktopHeight = desktopDesc.Height;
    return true;
}

bool DesktopCapture::AcquireFrame(uint32_t screenSizePercent, int32_t stereoSeparationPx) {
    if (!m_duplication) {
        return false;
    }

    ComPtr<IDXGIResource> desktopResource;
    DXGI_OUTDUPL_FRAME_INFO frameInfo = {};
    const HRESULT hr = m_duplication->AcquireNextFrame(0, &frameInfo, &desktopResource);
    if (hr == DXGI_ERROR_WAIT_TIMEOUT) {
        return false;
    }
    if (FAILED(hr)) {
        Error("DesktopCast: AcquireNextFrame failed (%x).\n", hr);
        return false;
    }

    ComPtr<ID3D11Texture2D> desktopTexture;
    if (FAILED(desktopResource->QueryInterface(IID_PPV_ARGS(&desktopTexture)))) {
        m_duplication->ReleaseFrame();
        return false;
    }

    D3D11_TEXTURE2D_DESC desktopDesc = {};
    desktopTexture->GetDesc(&desktopDesc);

    // Grow the canvas so that the desktop only covers `scale` of it, which keeps the aspect ratio
    // and leaves the black margin seen in the headset.
    const float scale = std::min<float>(1.0f, std::max<float>(0.05f, screenSizePercent / 100.0f));
    const uint32_t canvasWidth = (uint32_t)std::ceil(desktopDesc.Width / scale);
    const uint32_t canvasHeight = (uint32_t)std::ceil(desktopDesc.Height / scale);

    if (!m_canvasTextures[0] || canvasWidth != m_canvasWidth || canvasHeight != m_canvasHeight
        || desktopDesc.Width != m_desktopWidth || desktopDesc.Height != m_desktopHeight) {
        m_canvasWidth = canvasWidth;
        m_canvasHeight = canvasHeight;
        if (!PrepareCanvases(desktopTexture.Get())) {
            m_duplication->ReleaseFrame();
            return false;
        }
    }

    m_desktopTexture = desktopTexture;

    // The duplicated texture is only valid until ReleaseFrame, so it has to be copied right away.
    const float black[4] = { 0.0f, 0.0f, 0.0f, 1.0f };
    const int32_t offsetX = (int32_t)((m_canvasWidth - m_desktopWidth) / 2);
    const int32_t offsetY = (int32_t)((m_canvasHeight - m_desktopHeight) / 2);
    const int32_t separation = stereoSeparationPx > 0 ? stereoSeparationPx / 2 : 0;

    D3D11_BOX sourceBox = {};
    sourceBox.right = m_desktopWidth;
    sourceBox.bottom = m_desktopHeight;
    sourceBox.back = 1;

    auto context = m_d3dRender->GetContext();
    for (int i = 0; i < 2; i++) {
        context->ClearRenderTargetView(m_canvasRtv[i].Get(), black);

        const int32_t posX = offsetX + (i == 0 ? -separation : separation);
        if (posX < 0 || (uint32_t)(posX + m_desktopWidth) > m_canvasWidth) {
            continue;
        }
        context->CopySubresourceRegion(
            m_canvasTextures[i].Get(), 0, posX, offsetY, 0, m_desktopTexture.Get(), 0, &sourceBox
        );
    }

    m_duplication->ReleaseFrame();
    m_haveFrame = true;
    return true;
}

bool DesktopCapture::GetCanvas(int eye, ID3D11Texture2D** canvas) const {
    if (!m_haveFrame || !m_canvasTextures[eye]) {
        return false;
    }

    *canvas = m_canvasTextures[eye].Get();
    return true;
}
