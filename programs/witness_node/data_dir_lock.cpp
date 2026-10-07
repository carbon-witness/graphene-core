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
#include "data_dir_lock.hpp"

#include <boost/filesystem.hpp>

#include <cstring>
#include <string>

#ifdef _WIN32
# ifndef NOMINMAX
#  define NOMINMAX
# endif
# ifndef WIN32_LEAN_AND_MEAN
#  define WIN32_LEAN_AND_MEAN
# endif
# include <windows.h>
#else
# include <cerrno>
# include <fcntl.h>
# include <sys/file.h>
# include <unistd.h>
#endif

namespace graphene { namespace witness_node {

namespace {

const char* const LOCK_FILE = "witness_node.lock";

std::string in_use_message( const std::string& data_dir, const std::string& holder_pid )
{
   std::string who = holder_pid.empty() ? "Another witness_node" : "Another witness_node (PID " + holder_pid + ")";
   return who + " is already using the data directory " + data_dir +
          ". Stop it first: two nodes on one data directory corrupt its database.";
}

std::string trimmed( std::string s )
{
   while( !s.empty() && ( s.back() == '\n' || s.back() == '\r' || s.back() == ' ' || s.back() == '\0' ) )
      s.pop_back();
   return s;
}

} // anonymous namespace

#ifdef _WIN32

// The PID is written at the start of the file; the lock covers a byte far past it, so a second node can
// still read who holds the directory.
static const DWORD LOCK_OFFSET_HIGH = 1;

data_dir_lock::~data_dir_lock()
{
   if( _file != nullptr )
      CloseHandle( _file ); // also releases the lock
}

bool data_dir_lock::acquire( const std::string& data_dir, std::string& error )
{
   boost::system::error_code ec;
   boost::filesystem::create_directories( data_dir, ec );
   const std::wstring path = ( boost::filesystem::path( data_dir ) / LOCK_FILE ).wstring();
   HANDLE f = CreateFileW( path.c_str(), GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr,
                           OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr );
   if( f == INVALID_HANDLE_VALUE )
   {
      error = "Cannot open " + boost::filesystem::path( path ).string() + " (Windows error " +
              std::to_string( GetLastError() ) + ")";
      return false;
   }
   OVERLAPPED at = {};
   at.OffsetHigh = LOCK_OFFSET_HIGH;
   if( !LockFileEx( f, LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY, 0, 1, 0, &at ) )
   {
      char buf[32] = {};
      DWORD got = 0;
      ReadFile( f, buf, sizeof( buf ) - 1, &got, nullptr );
      CloseHandle( f );
      error = in_use_message( data_dir, trimmed( std::string( buf, got ) ) );
      return false;
   }
   const std::string pid = std::to_string( GetCurrentProcessId() ) + "\n";
   DWORD written = 0;
   SetFilePointer( f, 0, nullptr, FILE_BEGIN );
   WriteFile( f, pid.data(), (DWORD)pid.size(), &written, nullptr );
   SetEndOfFile( f );
   _file = f;
   return true;
}

#else

data_dir_lock::~data_dir_lock()
{
   if( _fd >= 0 )
      close( _fd ); // also releases the lock
}

bool data_dir_lock::acquire( const std::string& data_dir, std::string& error )
{
   boost::system::error_code ec;
   boost::filesystem::create_directories( data_dir, ec );
   const std::string path = ( boost::filesystem::path( data_dir ) / LOCK_FILE ).string();
   int fd = open( path.c_str(), O_RDWR | O_CREAT | O_CLOEXEC, 0644 );
   if( fd < 0 )
   {
      error = "Cannot open " + path + ": " + std::strerror( errno );
      return false;
   }
   if( flock( fd, LOCK_EX | LOCK_NB ) != 0 )
   {
      char buf[32] = {};
      ssize_t got = pread( fd, buf, sizeof( buf ) - 1, 0 );
      close( fd );
      error = in_use_message( data_dir, trimmed( std::string( buf, got > 0 ? got : 0 ) ) );
      return false;
   }
   const std::string pid = std::to_string( getpid() ) + "\n";
   if( ftruncate( fd, 0 ) == 0 )
      pwrite( fd, pid.data(), pid.size(), 0 );
   _fd = fd;
   return true;
}

#endif

} } // graphene::witness_node
