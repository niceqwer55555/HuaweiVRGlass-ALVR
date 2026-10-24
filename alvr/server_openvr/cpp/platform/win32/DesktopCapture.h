#pragma once

// Desktop duplication based capture, used by the optional desktop cast mode.
// The captured frame is copied into a black canvas that keeps the desktop aspect ratio, so the
// client sees a floating flat screen instead of a stretched picture. Each eye owns its own canvas
// because the optional stereo separation copies them at slightly different offsets.

#include <cstdint>
#include <d3d11.h>
#include <dxgi1_2.h>
#include <wrl.h>

using Microsoft::WRL::ComPtr;

class DesktopCapture {
public:
    explicit DesktopCapture(class CD3DRender* d3dRender);

    bool Initialize(uint32_t monitorIndex);

    void Stop();

    // Grabs the newest desktop frame and refreshes both canvases. Returns true when a new frame was
    // available; the previously copied canvas stays valid otherwise.
    bool AcquireFrame(uint32_t screenSizePercent, int32_t stereoSeparationPx);

    // Returns the canvas for one eye, or false if no desktop frame has ever been captured.
    bool GetCanvas(int eye, ID3D11Texture2D** canvas) const;

    bool IsReady() const { return m_duplication != nullptr; }

private:
    bool PrepareCanvases(ID3D11Texture2D* desktopTexture);

    class CD3DRender* m_d3dRender = nullptr;
    ComPtr<IDXGIOutputDuplication> m_duplication;
    ComPtr<ID3D11Texture2D> m_desktopTexture;
    ComPtr<ID3D11Texture2D> m_canvasTextures[2];
    ComPtr<ID3D11RenderTargetView> m_canvasRtv[2];
    uint32_t m_canvasWidth = 0;
    uint32_t m_canvasHeight = 0;
    uint32_t m_desktopWidth = 0;
    uint32_t m_desktopHeight = 0;
    bool m_haveFrame = false;
};
