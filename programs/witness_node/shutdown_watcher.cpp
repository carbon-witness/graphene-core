/*
 * Copyright (c) 2026 contributors.
 *
 * The MIT License
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in
 * all copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
 * THE SOFTWARE.
 */
#ifdef _WIN32

#include "shutdown_watcher.hpp"

#include <cstdio>
#include <stdexcept>
#include <vector>

#ifndef NOMINMAX
# define NOMINMAX
#endif
#ifndef WIN32_LEAN_AND_MEAN
# define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>

namespace graphene { namespace witness_node {

namespace {

std::runtime_error last_error( const std::string& what )
{
   return std::runtime_error( what + " (Windows error " + std::to_string( GetLastError() ) + ")" );
}

std::wstring to_wide( const std::string& s )
{
   int n = MultiByteToWideChar( CP_UTF8, MB_ERR_INVALID_CHARS, s.data(), (int)s.size(), nullptr, 0 );
   if( n <= 0 )
      throw last_error( "Invalid UTF-8 in --shutdown-event name" );
   std::wstring w( n, L'\0' );
   MultiByteToWideChar( CP_UTF8, MB_ERR_INVALID_CHARS, s.data(), (int)s.size(), &w[0], n );
   return w;
}

void close( void*& h )
{
   if( h != nullptr )
      CloseHandle( h );
   h = nullptr;
}

} // anonymous namespace

shutdown_watcher::shutdown_watcher( const std::string& event_name, uint32_t parent_pid )
   : _parent_pid( parent_pid ), _event_name( event_name )
{
   try
   {
      _stop = CreateEventW( nullptr, TRUE, FALSE, nullptr );
      if( _stop == nullptr )
         throw last_error( "Cannot create the internal stop event" );

      if( !event_name.empty() )
      {
         // The GUI owns the event; the node never creates it, so a typo in the name fails here
         // instead of leaving a node no one can stop.
         _event = OpenEventW( SYNCHRONIZE, FALSE, to_wide( event_name ).c_str() );
         if( _event == nullptr )
            throw last_error( "Cannot open shutdown event \"" + event_name + "\"" );
      }

      if( parent_pid != 0 )
      {
         _parent = OpenProcess( SYNCHRONIZE, FALSE, parent_pid );
         if( _parent == nullptr )
            throw last_error( "Cannot open parent process " + std::to_string( parent_pid ) );
      }
   }
   catch( ... )
   {
      close( _parent );
      close( _event );
      close( _stop );
      throw;
   }
}

shutdown_watcher::~shutdown_watcher()
{
   if( _stop != nullptr )
      SetEvent( _stop );
   if( _thread.joinable() )
      _thread.join();
   close( _parent );
   close( _event );
   close( _stop );
}

void shutdown_watcher::start( std::function<void(const std::string&)> on_shutdown )
{
   if( _event == nullptr && _parent == nullptr )
      return;

   _thread = std::thread( [this, on_shutdown]() {
      // _stop goes first so a destructor call wins over a source that fires at the same time
      std::vector<HANDLE> handles{ _stop };
      if( _event != nullptr )
         handles.push_back( _event );
      if( _parent != nullptr )
         handles.push_back( _parent );

      DWORD r = WaitForMultipleObjects( (DWORD)handles.size(), handles.data(), FALSE, INFINITE );
      if( r < WAIT_OBJECT_0 || r >= WAIT_OBJECT_0 + handles.size() )
      {
         on_shutdown( "wait for the shutdown event failed (Windows error "
                      + std::to_string( GetLastError() ) + ")" );
         return;
      }

      HANDLE fired = handles[r - WAIT_OBJECT_0];
      if( fired == _stop )
         return;
      if( fired == _event )
         on_shutdown( "shutdown event \"" + _event_name + "\" was signalled" );
      else
         on_shutdown( "parent process " + std::to_string( _parent_pid ) + " exited" );
   } );
}

namespace {

// Process-wide and never freed: a handler thread Windows started may still use them while main returns
std::function<void(const std::string&)>* console_on_close = nullptr;
HANDLE console_done = nullptr;

BOOL WINAPI console_handler( DWORD type )
{
   const char* what;
   switch( type )
   {
   case CTRL_CLOSE_EVENT: what = "console window was closed"; break;
   case CTRL_BREAK_EVENT: what = "Ctrl+Break pressed"; break;
   default: return FALSE; // Ctrl+C stays with the SIGINT handler
   }
   (*console_on_close)( what );
   WaitForSingleObject( console_done, INFINITE );
   return TRUE;
}

} // anonymous namespace

void install_console_close_handler( std::function<void(const std::string&)> on_close )
{
   if( console_on_close != nullptr )
      return;
   console_done = CreateEventW( nullptr, TRUE, FALSE, nullptr );
   if( console_done == nullptr )
      throw last_error( "Cannot create the console shutdown event" );
   console_on_close = new std::function<void(const std::string&)>( std::move( on_close ) );
   if( !SetConsoleCtrlHandler( console_handler, TRUE ) )
      throw last_error( "Cannot install the console handler" );
}

void console_close_handled()
{
   if( console_done != nullptr )
      SetEvent( console_done );
}

void pause_if_console_closes_on_exit()
{
   DWORD processes[2];
   DWORD mode = 0;
   if( GetConsoleProcessList( processes, 2 ) != 1 || GetConsoleWindow() == nullptr
       || !GetConsoleMode( GetStdHandle( STD_INPUT_HANDLE ), &mode ) )
      return;
   std::fputs( "\nPress Enter to close this window.", stderr );
   std::fflush( stderr );
   std::getchar();
}

} } // graphene::witness_node

#endif // _WIN32
