import os
import shutil

# project: "Markdown Viewer"
# target-dir: "c:/dev/Markdown-viewer"
# copy: true

def detect_source_and_target(source_path):
    unit_switch = ["c:", "x:"]
    if source_path[0:2].lower() == unit_switch[0].lower():
        return unit_switch[1].lower() + source_path[2:]
    elif source_path[0:2].lower() == unit_switch[1].lower():
        return unit_switch[0].lower() + source_path[2:]
    return None

def copy_project(source_dir, dest_dir):
    # Patterns to ignore during the copy process
    # This prevents copying heavy cache folders or unnecessary files
    ignored_patterns = shutil.ignore_patterns(
        'node_modules', 
        '.git',
        '.idea',
        '.vscode',
        '.next',
        'dist', 
        'build', 
        '.DS_Store',
        '__pycache__',
        '.vite'
    )

    print(f"Starting to copy from '{source_dir}' to '{dest_dir}'...")

    count_folders = 0
    count_files_copied = 0

    try:
        # Check if destination directory exists
        if not os.path.exists(dest_dir):
            os.makedirs(dest_dir)
            count_folders += 1
            print(f"Created destination directory '{dest_dir}'")

        # copy files in source_dir if file is newer or modified
        for root, dirs, files in os.walk(source_dir):
            ignored_names = ignored_patterns(root, dirs + files)
            dirs[:] = [d for d in dirs if d not in ignored_names]

            for file in files:
                if file in ignored_names:
                    continue
                
                source_file = os.path.join(root, file)
                dest_file = os.path.join(root.replace(source_dir, dest_dir), file)
                
                # Check if we need to copy (file is newer or doesn't exist)
                needs_copy = True
                if os.path.exists(dest_file):
                    src_mtime = os.path.getmtime(source_file)
                    dst_mtime = os.path.getmtime(dest_file)
                    if dst_mtime >= src_mtime - 0.001:
                        needs_copy = False
                
                if needs_copy:
                    dest_file_dir = os.path.dirname(dest_file)
                    if not os.path.exists(dest_file_dir):
                        os.makedirs(dest_file_dir)
                        count_folders += 1
                    shutil.copy2(source_file, dest_file)
                    count_files_copied += 1
                    print(f"Copied {source_file} to {dest_file}")
                    
        print("Copy completed successfully!")
        print(f"Created {count_folders} new folders and copied {count_files_copied} files")
    except Exception as e:
        print(f"An error occurred during copying: {e}")

if __name__ == "__main__":
    script_path = os.path.abspath(__file__)
    source_dir = os.path.dirname(script_path)
    # detect source and target
    dest_dir = detect_source_and_target(source_dir)
    # Make tree copy project from source_dir to dest_dir
    if dest_dir:
        print(f"Starting copy of '{source_dir}' to '{dest_dir}'...")
        copy_project(source_dir, dest_dir)
    else:
        print(f"Could not detect target directory for '{source_dir}'...")
