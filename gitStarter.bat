# 0. Remove all previous
git rm -r -f --cached .

# 1. Start a new repository
git init

# 2. Add files (Git will now respect your .gitignore)
git add .

# 3. Verify the secret file is NOT listed
git status

git ls-files


# 4. Create your first commit
git commit -m "Fresh restart: Initial commit without secrets"