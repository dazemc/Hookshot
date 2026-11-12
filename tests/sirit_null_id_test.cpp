// Sirit's undefined operand: an Id with no SPIR-V definition must throw, naming the failure, instead of
// std::abort() ending the game with nothing in the log. No game or GPU is needed.
#include "sirit/sirit.h"
#include "sirit/src/stream.h"

#include <cstdio>
#include <stdexcept>

int main() {
    Sirit::u32 bound = 0;
    Sirit::Stream stream(&bound);
    try {
        stream << Sirit::Id{};
    } catch (const std::runtime_error& e) {
        if (std::string_view{e.what()}.find("null Id") == std::string_view::npos) {
            std::printf("FAIL: the error does not name the null Id: %s\n", e.what());
            return 1;
        }
        std::printf("sirit_null_id: ok\n");
        return 0;
    }
    std::printf("FAIL: an undefined operand was accepted\n");
    return 1;
}
