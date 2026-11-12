// (hookshot) When the lost-device report asks VK_EXT_device_fault (vk_draw.cpp checkpoints_report).
// tests/device_fault_query_test.cpp
//
// RX_VK_FAULT_INFO=1 asks for the fault info by hand, at once and again at the end. Without the NVIDIA checkpoints and the
// AMD buffer markers nothing else says which draw stopped the GPU, so the report asks automatically on a GPU where that
// call is safe: AMD and Intel (RADV, ANV, the Windows drivers) get the faulting address and the vendor fault info from the
// first lost device, with no run to reproduce it with RX_VK_FAULT_INFO=1.
//
// On NVIDIA the call has not come back before the checkpoint data finished reporting, so it stays off there even when
// RX_VK_CHECKPOINTS=0 took the checkpoints away. RX_VK_FAULT_AUTO=0 turns the automatic ask off on any GPU; it does not
// turn off RX_VK_FAULT_INFO.
#pragma once
#include "compat/hookshot_env.h"

namespace device_fault_query {

/// RX_VK_FAULT_INFO: the switch that asks by hand, at once and at the end.
inline bool forced(const char* info_env) { return info_env && !env_value_off(info_env); }

/// The automatic ask: no NVIDIA checkpoints, no buffer markers, not NVIDIA, and RX_VK_FAULT_AUTO not off.
inline bool automatic(bool nvidia, bool checkpoints, bool markers, const char* auto_env) {
    return !nvidia && !checkpoints && !markers && !(auto_env && env_value_off(auto_env));
}

/// Whether checkpoints_report's last report asks for the fault info: the switch, or the automatic ask.
inline bool wanted(bool nvidia, bool checkpoints, bool markers, const char* info_env, const char* auto_env) {
    return forced(info_env) || automatic(nvidia, checkpoints, markers, auto_env);
}

} // namespace device_fault_query
