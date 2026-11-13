# Fcitx5 插件构建。
# 前置条件：先构建 Rust 核心引擎（core/），再以变量传入产物路径：
#   cmake -B build -S addon \
#     -DQINGCI_CORE_STATIC_LIB=<path>/libqingci_core.a \
#     -DQINGCI_CORE_DICT_PATH=<path>/qingci.dic
cmake_minimum_required(VERSION 3.16)
project(fcitx5-qingci VERSION 0.2.8 LANGUAGES CXX)

include(GNUInstallDirs)
find_package(ECM REQUIRED)
list(APPEND CMAKE_MODULE_PATH ${ECM_MODULE_PATH})
find_package(Fcitx5Core REQUIRED)
find_package(Fcitx5Utils REQUIRED)
include("${FCITX_INSTALL_CMAKECONFIG_DIR}/Fcitx5Utils/Fcitx5CompilerSettings.cmake")

if(NOT QINGCI_CORE_STATIC_LIB)
    message(FATAL_ERROR "请通过 -DQINGCI_CORE_STATIC_LIB= 传入 libqingci_core.a 路径（先构建 core/）")
endif()
if(NOT QINGCI_CORE_DICT_PATH)
    # 默认安装全量词库；演示词库为 core/data/qingci.dic
    set(QINGCI_CORE_DICT_PATH "${CMAKE_CURRENT_SOURCE_DIR}/../core/data/qingci-full.dic")
endif()
# 运行时词库路径必须指向安装位置（而非构建目录）；
# 源码运行调试可用 -DQINGCI_RUNTIME_DICT_PATH 覆盖。
if(NOT QINGCI_RUNTIME_DICT_PATH)
    set(QINGCI_RUNTIME_DICT_PATH
        "${FCITX_INSTALL_PKGDATADIR}/qingci/qingci-full.dic")
endif()

add_fcitx5_addon(qingci src/qingci.cpp)
target_link_libraries(qingci
    Fcitx5::Core
    ${QINGCI_CORE_STATIC_LIB}
    dl
    m
)
target_compile_definitions(qingci PRIVATE
    QINGCI_DICT_PATH="${QINGCI_RUNTIME_DICT_PATH}"
)

install(TARGETS qingci DESTINATION "${FCITX_INSTALL_LIBDIR}/fcitx5")
install(PROGRAMS "${CMAKE_CURRENT_SOURCE_DIR}/../scripts/qingci-setup"
        DESTINATION "${CMAKE_INSTALL_BINDIR}")
install(FILES data/qingci.conf
        DESTINATION "${FCITX_INSTALL_PKGDATADIR}/inputmethod")
install(FILES data/qingci-addon.conf
        RENAME qingci.conf
        DESTINATION "${FCITX_INSTALL_PKGDATADIR}/addon")
install(FILES "${QINGCI_CORE_DICT_PATH}"
        DESTINATION "${FCITX_INSTALL_PKGDATADIR}/qingci")
install(FILES data/qingci.svg
        DESTINATION "${CMAKE_INSTALL_DATADIR}/icons/hicolor/scalable/apps"
        RENAME qingci.svg)
install(FILES themes/qingci-dark/theme.conf
              themes/qingci-dark/background.png
              themes/qingci-dark/highlight.png
              themes/qingci-dark/prev.svg
              themes/qingci-dark/next.svg
              themes/qingci-dark/arrow.svg
              themes/qingci-dark/radio.svg
        DESTINATION "${FCITX_INSTALL_PKGDATADIR}/themes/qingci-dark")
