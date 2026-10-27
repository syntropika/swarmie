#include <windows.h>
#include <audioclient.h>
#include <mfidl.h>
#include <mmdeviceapi.h>
#include <oleacc.h>

// Some Windows SDK import libraries omit these declarations' definitions.
// Resolve each interface and class identity from its SDK declaration.
extern "C" const CLSID CLSID_AccPropServices = {0xb5f8350b, 0x0548, 0x48b1, {0xa6, 0xee, 0x88, 0xbd, 0x00, 0xb4, 0xa5, 0xe7}};
extern "C" const IID IID_IMFMediaSource = __uuidof(IMFMediaSource);
extern "C" const IID IID_IMFAudioStreamVolume = __uuidof(IMFAudioStreamVolume);
extern "C" const CLSID CLSID_MMDeviceEnumerator = __uuidof(MMDeviceEnumerator);
extern "C" const IID IID_IMMDeviceEnumerator = __uuidof(IMMDeviceEnumerator);
extern "C" const IID IID_IAudioClient = __uuidof(IAudioClient);
extern "C" const IID IID_IAudioCaptureClient = __uuidof(IAudioCaptureClient);
extern "C" const IID IID_IMFPresentationClock = __uuidof(IMFPresentationClock);
extern "C" const IID IID_IMFAsyncCallback = __uuidof(IMFAsyncCallback);
extern "C" const IID IID_IActivateAudioInterfaceCompletionHandler = __uuidof(IActivateAudioInterfaceCompletionHandler);

// The MSAA name-property GUID specified by the Windows SDK's oleacc.h.
extern "C" const MSAAPROPID PROPID_ACC_NAME = {0x608d3df8, 0x8128, 0x4aa7, {0xa4, 0x28, 0xf5, 0x5e, 0x49, 0x26, 0x72, 0x91}};
