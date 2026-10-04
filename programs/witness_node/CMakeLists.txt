set( WITNESS_NODE_SOURCES main.cpp shutdown_watcher.cpp data_dir_lock.cpp )
if( WIN32 )
  # The version shown in the file's properties: 1.2.1 -> FILEVERSION 1,2,1,0; the build string as product version
  # Compiled on its own: the target's flags (add_definitions(-fPIC) at the top level) would reach windres too
  string( REGEX MATCH "^([0-9]+)\\.([0-9]+)\\.([0-9]+)" _version "${GRAPHENE_VERSION}" )
  set( WITNESS_NODE_VERSION_COMMAS "${CMAKE_MATCH_1},${CMAKE_MATCH_2},${CMAKE_MATCH_3}" )
  configure_file( witness_node.rc.in "${CMAKE_CURRENT_BINARY_DIR}/witness_node.rc" @ONLY )
  if( NOT CMAKE_RC_COMPILER )
    find_program( CMAKE_RC_COMPILER NAMES ${CMAKE_C_COMPILER_TARGET}-windres x86_64-w64-mingw32-windres windres )
  endif()
  add_custom_command( OUTPUT "${CMAKE_CURRENT_BINARY_DIR}/witness_node_rc.o"
                      COMMAND ${CMAKE_RC_COMPILER} -O coff "${CMAKE_CURRENT_BINARY_DIR}/witness_node.rc"
                              "${CMAKE_CURRENT_BINARY_DIR}/witness_node_rc.o"
                      DEPENDS "${CMAKE_CURRENT_BINARY_DIR}/witness_node.rc" )
  list( APPEND WITNESS_NODE_SOURCES "${CMAKE_CURRENT_BINARY_DIR}/witness_node_rc.o" )
endif()
add_executable( witness_node ${WITNESS_NODE_SOURCES} )
if( UNIX AND NOT APPLE )
  set(rt_library rt )
endif()

find_package( Gperftools QUIET )
if( GPERFTOOLS_FOUND )
    message( STATUS "Found gperftools; compiling witness_node with TCMalloc")
    list( APPEND PLATFORM_SPECIFIC_LIBS tcmalloc )
endif()

# We have to link against graphene_debug_witness because deficiency in our API infrastructure doesn't allow plugins to be fully abstracted #246
target_link_libraries( witness_node

PRIVATE graphene_app graphene_delayed_node graphene_account_history graphene_elasticsearch graphene_market_history graphene_grouped_orders graphene_witness graphene_chain graphene_debug_witness graphene_egenesis_full graphene_snapshot graphene_es_objects
        graphene_api_helper_indexes
        fc ${CMAKE_DL_LIBS} ${PLATFORM_SPECIFIC_LIBS} )

install( TARGETS
   witness_node

   RUNTIME DESTINATION bin
   LIBRARY DESTINATION lib
   ARCHIVE DESTINATION lib
)
