// SPDX-License-Identifier: GPL-2.0-or-later
// Integration test against actual libobs, with no display or audio hardware.
#include <obs.h>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstring>
#include <thread>
#include <iostream>
#include <arpa/inet.h>
#include <sys/socket.h>
#include <unistd.h>
extern "C" bool obs_module_load();
extern "C" void obs_module_unload();
static std::atomic<unsigned> received{0};
static void capture(void*, obs_source_t*, const audio_data* audio, bool) {
    const auto* samples = reinterpret_cast<const float*>(audio->data[0]);
    for (unsigned i=0;i<audio->frames;++i) {
        if (std::isfinite(samples[i]) && std::abs(samples[i])>0.01f) {received.fetch_add(1);break;}
    }
}
int main(int argc, char**) {
    if (!obs_startup("en-US", nullptr, nullptr)) return 1;
    obs_audio_info info{};info.samples_per_sec=48000;info.speakers=SPEAKERS_STEREO;
    if (!obs_reset_audio(&info) || !obs_module_load()) {obs_shutdown();return 2;}
    auto* source=obs_source_create_private("monitor_mix_receiver", "test", nullptr);
    if (!source) {obs_shutdown();return 3;}
    obs_source_add_audio_capture_callback(source,capture,nullptr);
    if (argc > 1) {
        std::cout<<"READY: libobs receiver bound"<<std::endl;
        for (unsigned i=0;i<700 && received.load()==0;++i)std::this_thread::sleep_for(std::chrono::milliseconds(10));
        obs_source_remove_audio_capture_callback(source,capture,nullptr);
        obs_source_release(source);obs_module_unload();obs_shutdown();
        if (!received.load()) {std::cerr<<"External app audio did not arrive\n";return 5;}
        std::cout<<"PASS: app master reached actual libobs capture callbacks\n";
        return 0;
    }
    const int sock=socket(AF_INET,SOCK_DGRAM,0);
    sockaddr_in addr{};addr.sin_family=AF_INET;addr.sin_port=htons(17840);addr.sin_addr.s_addr=htonl(INADDR_LOOPBACK);
    unsigned char packet[16+256*8]{};std::memcpy(packet,"MMX1",4);
    const uint32_t rate=48000,frames=256;std::memcpy(packet+8,&rate,4);std::memcpy(packet+12,&frames,4);
    for (unsigned i=0;i<512;++i) {const float v=0.1f;std::memcpy(packet+16+i*4,&v,4);}
    // A malformed datagram must be ignored, then valid data must still pass.
    sendto(sock,"bad",3,0,reinterpret_cast<sockaddr*>(&addr),sizeof(addr));
    for (uint32_t i=0;i<12;++i) {
        std::memcpy(packet+4,&i,4);sendto(sock,packet,sizeof(packet),0,reinterpret_cast<sockaddr*>(&addr),sizeof(addr));
        std::this_thread::sleep_for(std::chrono::milliseconds(6));
    }
    close(sock);
    for (unsigned i=0;i<50 && received.load()==0;++i)std::this_thread::sleep_for(std::chrono::milliseconds(10));
    obs_source_remove_audio_capture_callback(source,capture,nullptr);
    obs_source_release(source);obs_module_unload();obs_shutdown();
    if (!received.load()) {std::cerr<<"No nonzero audio reached libobs\n";return 4;}
    std::cout<<"PASS: malformed packet ignored; stereo UDP audio reached actual libobs capture callbacks\n";
}
